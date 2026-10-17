//! Authenticated client observations; lost write replies never authorize automatic replay.
use super::{hash, keys, pin, valid_input};
use crate::{
    OperatorContext, RunControl,
    request::Request,
    result::{OperationResult, Status},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn initial(request: &Request) -> Value {
    json!({"format_version":1,"action":request.operation.trim_start_matches("change."),"runner":null,"principal":null,"requester":request.inputs.get("requester"),"request_id":request.inputs.get("request_id"),"target":null,"plan":null,"plan_sha256":null,"approval_id":null,"state":"not_found","effect_outcome":"not_attempted","remote_error":null})
}
fn writes(method: &str) -> bool {
    matches!(method, "plan" | "approve" | "apply" | "reconcile")
}
fn uncertain(result: &mut OperationResult) {
    result.execution.status = Status::Unknown;
    result.effect_outcome = "unknown".into();
    result.data["state"] = json!("unknown");
    result.data["effect_outcome"] = json!("unknown");
}
pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    result.data = initial(request);
    result.record_mode = match request.record.as_str() {
        "always" => "always",
        "auto" => "auto",
        _ => "never",
    };
    result.coverage.scope = "Exact approved rule-file replacement and retained identity observations; no service reload or service-health assessment.".into();
    #[cfg(target_os = "linux")]
    let outcome = call(request, control, context, start, result);
    #[cfg(not(target_os = "linux"))]
    let outcome: Result<(), &'static str> = {
        let _ = (control, context, start);
        Err("change_platform_unsupported")
    };
    if let Err(code) = outcome {
        if result.execution.status != Status::Unknown {
            result.execution.status = match code {
                "runner_deadline_exceeded" => Status::TimedOut,
                "runner_cancelled" => Status::Cancelled,
                "change_platform_unsupported" => Status::Unsupported,
                _ => Status::Denied,
            };
        }
        result.error(code, "The change operation could not be established. Inspect or reconcile the original request identity; never repeat replacement to repair a lost response.");
    }
}
#[cfg(target_os = "linux")]
fn call(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) -> Result<(), &'static str> {
    let method = request.operation.trim_start_matches("change.");
    if !valid_input(method, &request.inputs) {
        return Err("invalid_change_input");
    }
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    let prepared = crate::runner::prepare_client(
        context
            .runner_config_path
            .as_deref()
            .ok_or("runner_config_required")?,
        control,
        deadline,
    )
    .map_err(|e| e.code)?;
    if prepared.identity["runner_id"] != request.target.id {
        return Err("runner_identity_mismatch");
    }
    let wire = serde_json::to_vec(
        &json!({"protocol":1,"method":request.operation,"inputs":request.inputs}),
    )
    .unwrap();
    let bytes = match crate::runner::exchange_change(&prepared, &wire, control, deadline) {
        Ok(bytes) => bytes,
        Err((code, sent)) => {
            if sent && writes(method) {
                uncertain(result);
            }
            return Err(code);
        }
    };
    let response = crate::request::parse_bounded_json(&bytes, 48).ok();
    let Some(response) = response.filter(|v| valid_response(v, request, &prepared.identity)) else {
        if writes(method) {
            uncertain(result);
        }
        return Err("change_invalid_response");
    };
    result.data = response["data"].clone();
    result.effect_outcome = match result.data["effect_outcome"].as_str() {
        Some("verified") => "succeeded",
        Some("unknown") => "unknown",
        _ => "not_attempted",
    }
    .into();
    if response["ok"] != true {
        result.execution.status = if result.effect_outcome == "unknown" {
            Status::Unknown
        } else {
            // Retained data can describe an earlier verified attempt; this refused
            // invocation did not establish a new effect.
            result.effect_outcome = "not_attempted".into();
            Status::Denied
        };
        result.error("change_remote_refusal", "The authenticated runner refused the change action; inspect remote_error and the retained plan state.");
    } else if result.data["state"] == "unknown" || result.data["state"] == "dispatching" {
        result.execution.status = Status::Unknown;
        result.coverage.state = "not_established".into();
        result.error("change_outcome_unknown", "The original attempt has no verified current outcome. Reconcile the retained identity; replacement must not be repeated.");
    }
    // Omit the complete plan as a unit, preserving the digest and recovery identity.
    if serde_json::to_vec(&result.data["plan"]).unwrap().len()
        > result.effective_limits.max_output_bytes
    {
        result.data["plan"] = Value::Null;
        if result.execution.status == Status::Succeeded {
            result.execution.status = Status::Partial;
        }
        result.coverage.state = "partial".into();
        result.coverage.limitations.push("Exact plan omitted by the output detail limit; request status with a larger bound before approving.".into());
    }
    Ok(())
}
fn nullable_id(v: &Value) -> bool {
    v.is_null() || super::identifier(v)
}
fn nullable_pin(v: &Value) -> bool {
    v.is_null() || v.as_str().is_some_and(pin)
}
fn runner(v: &Value) -> bool {
    keys(v, &["id", "server_certificate_sha256", "policy_sha256"])
        && v["id"]
            .as_str()
            .is_some_and(crate::request::valid_operation)
        && ["server_certificate_sha256", "policy_sha256"]
            .iter()
            .all(|k| v[*k].as_str().is_some_and(pin))
}
fn identity(v: &Value) -> bool {
    keys(
        v,
        &[
            "dev",
            "ino",
            "uid",
            "gid",
            "mode",
            "nlink",
            "size",
            "ctime_sec",
            "ctime_nsec",
            "mtime_sec",
            "mtime_nsec",
        ],
    ) && ["dev", "ino", "uid", "gid", "mode", "nlink", "size"]
        .iter()
        .all(|k| v[*k].as_u64().is_some())
        && ["ctime_sec", "mtime_sec"]
            .iter()
            .all(|k| v[*k].as_i64().is_some())
        && ["ctime_nsec", "mtime_nsec"].iter().all(|k| {
            v[*k]
                .as_i64()
                .is_some_and(|n| (0..1_000_000_000).contains(&n))
        })
}
fn strings(v: &Value) -> bool {
    v.as_array().is_some_and(|a| a.iter().all(Value::is_string))
}
fn valid_times(start: &Value, end: &Value) -> bool {
    let parse = |v: &Value| {
        v.as_str().and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        })
    };
    match (parse(start), parse(end)) {
        (Some(a), Some(b)) => b >= a,
        _ => false,
    }
}
pub(super) fn valid_plan(plan: &Value) -> bool {
    if !keys(
        plan,
        &[
            "format_version",
            "kind",
            "requester",
            "requester_certificate_sha256",
            "request_id",
            "target",
            "runner",
            "ledger",
            "original_identity",
            "original_base64",
            "original_sha256",
            "proposed_base64",
            "proposed_sha256",
            "validator_binding_sha256",
            "implementation_sha256",
            "validation",
            "created_at",
            "expires_at",
            "expires_in_seconds",
            "readback",
            "recovery",
        ],
    ) || plan["format_version"] != 1
        || plan["kind"] != "prometheus-rules-file-replacement"
        || !super::identifier(&plan["requester"])
        || !super::identifier(&plan["request_id"])
        || !runner(&plan["runner"])
        || !keys(&plan["target"], &["id", "path"])
        || !super::identifier(&plan["target"]["id"])
        || !plan["target"]["path"].as_str().is_some_and(|p| {
            std::path::Path::new(p).is_absolute()
                && crate::request::bounded(p, 1, 1024)
                && !p.chars().any(char::is_control)
        })
        || !keys(&plan["ledger"], &["device", "inode"])
        || !["device", "inode"]
            .iter()
            .all(|k| plan["ledger"][*k].as_u64().is_some())
        || !keys(&plan["original_identity"], &["parent", "file", "sha256"])
        || !identity(&plan["original_identity"]["parent"])
        || !identity(&plan["original_identity"]["file"])
        || plan["original_identity"]["sha256"] != plan["original_sha256"]
        || ![
            "original_sha256",
            "proposed_sha256",
            "requester_certificate_sha256",
            "validator_binding_sha256",
            "implementation_sha256",
        ]
        .iter()
        .all(|k| plan[*k].as_str().is_some_and(pin))
        || plan["readback"] != "prepared_inode_and_exact_bytes"
        || plan["recovery"] != "new_approved_plan_using_original_bytes"
    {
        return false;
    }
    let (Some(created), Some(expires), Some(ttl)) = (
        plan["created_at"].as_u64(),
        plan["expires_at"].as_u64(),
        plan["expires_in_seconds"].as_u64(),
    ) else {
        return false;
    };
    if !(1..=900).contains(&ttl) || created.checked_add(ttl) != Some(expires) {
        return false;
    }
    for (field, digest) in [
        ("original_base64", "original_sha256"),
        ("proposed_base64", "proposed_sha256"),
    ] {
        let Some(bytes) = plan[field].as_str().and_then(super::decode) else {
            return false;
        };
        if hash(&bytes) != plan[digest] {
            return false;
        }
    }
    if plan["original_sha256"] == plan["proposed_sha256"] {
        return false;
    }
    let receipt = &plan["validation"];
    let validation = &receipt["data"];
    keys(
        receipt,
        &[
            "spec_version",
            "request_id",
            "run_id",
            "operation",
            "operation_version",
            "target",
            "resolved_target",
            "started_at",
            "finished_at",
            "duration_ms",
            "execution",
            "effect_outcome",
            "assessment",
            "coverage",
            "output",
            "data",
            "errors",
            "sources",
            "artifacts",
            "effective_limits",
            "record_mode",
        ],
    ) && receipt["spec_version"] == "0.1"
        && receipt["operation"] == "config.check"
        && receipt["operation_version"] == 1
        && super::identifier(&receipt["request_id"])
        && super::identifier(&receipt["run_id"])
        && receipt["target"] == json!({"kind":"local","id":"workstation"})
        && receipt["resolved_target"].is_object()
        && receipt["duration_ms"].as_u64().is_some()
        && valid_times(&receipt["started_at"], &receipt["finished_at"])
        && receipt["effect_outcome"] == "not_applicable"
        && receipt["record_mode"] == "never"
        && keys(
            &receipt["execution"],
            &["status", "child_exit_code", "signal"],
        )
        && receipt["execution"]["child_exit_code"] == 0
        && receipt["execution"]["signal"].is_null()
        && keys(&receipt["coverage"], &["state", "scope", "limitations"])
        && receipt["coverage"]["state"] == "complete"
        && receipt["coverage"]["scope"].is_string()
        && strings(&receipt["coverage"]["limitations"])
        && keys(
            &receipt["output"],
            &[
                "stdout",
                "stderr",
                "stdout_truncated",
                "stderr_truncated",
                "encoding",
            ],
        )
        && receipt["output"]["encoding"] == "utf-8"
        && receipt["output"]["stdout"].is_string()
        && receipt["output"]["stderr"].is_string()
        && receipt["sources"]
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_object))
        && receipt["artifacts"]
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_object))
        && keys(
            &receipt["effective_limits"],
            &["timeout_ms", "max_output_bytes"],
        )
        && receipt["effective_limits"]["timeout_ms"]
            .as_u64()
            .is_some_and(|n| (1..=300_000).contains(&n))
        && receipt["effective_limits"]["max_output_bytes"]
            .as_u64()
            .is_some_and(|n| (1024..=2_097_152).contains(&n))
        && keys(
            validation,
            &[
                "kind",
                "validator",
                "input",
                "implemented_rules",
                "unsupported_sections",
                "validation_state",
                "validation_outcome",
                "checked_rules",
                "findings",
                "findings_total",
                "diagnostics",
                "runtime",
                "cancellation_signal",
                "supervisor",
            ],
        )
        && keys(
            &validation["validator"],
            &[
                "id",
                "version",
                "checker_digest",
                "binding_digest",
                "upstream_revision",
            ],
        )
        && validation["validator"]["checker_digest"]
            .as_str()
            .is_some_and(pin)
        && validation["validator"]["upstream_revision"]
            .as_str()
            .is_some_and(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        && keys(&validation["input"], &["file", "digest", "bytes"])
        && validation["input"]["file"]
            .as_str()
            .is_some_and(|p| std::path::Path::new(p).is_absolute())
        && validation["input"]["bytes"].as_u64()
            == plan["proposed_base64"]
                .as_str()
                .and_then(super::decode)
                .map(|b| b.len() as u64)
        && strings(&validation["implemented_rules"])
        && strings(&validation["unsupported_sections"])
        && keys(
            &validation["diagnostics"],
            &["stdout", "stderr", "untrusted", "truncated"],
        )
        && validation["diagnostics"]["stdout"].is_string()
        && validation["diagnostics"]["stderr"].is_string()
        && validation["diagnostics"]["untrusted"] == true
        && keys(
            &validation["runtime"],
            &["executable", "executable_sha256", "revision", "version"],
        )
        && validation["runtime"]["executable"].is_string()
        && validation["runtime"]["executable_sha256"] == validation["validator"]["checker_digest"]
        && validation["runtime"]["revision"] == validation["validator"]["upstream_revision"]
        && validation["runtime"]["version"].is_string()
        && validation["cancellation_signal"].is_null()
        && validation["supervisor"]["cancellation_signal"].is_null()
        && validation["supervisor"]["cleanup"]["confirmed"] == true
        && validation["supervisor"]["cleanup"]["primary_reaped"] == true
        && validation["supervisor"]["cleanup"]["stdout_closed"] == true
        && validation["supervisor"]["cleanup"]["stderr_closed"] == true
        && receipt["execution"]["status"] == "succeeded"
        && receipt["assessment"] == "not_assessed"
        && receipt["errors"].as_array().is_some_and(Vec::is_empty)
        && receipt["output"]["stdout_truncated"] == false
        && receipt["output"]["stderr_truncated"] == false
        && validation["kind"] == "prometheus-rules"
        && validation["validator"]["id"] == "promtool"
        && validation["validator"]["version"] == 1
        && validation["validator"]["binding_digest"] == plan["validator_binding_sha256"]
        && validation["input"]["digest"] == plan["proposed_sha256"]
        && validation["validation_state"] == "complete"
        && validation["validation_outcome"] == "valid"
        && validation["checked_rules"].as_u64().is_some_and(|n| n > 0)
        && validation["findings_total"] == 0
        && validation["findings"].as_array().is_some_and(Vec::is_empty)
        && validation["diagnostics"]["truncated"] == false
}
fn valid_response(v: &Value, request: &Request, expected: &Value) -> bool {
    let d = &v["data"];
    let method = request.operation.trim_start_matches("change.");
    if !keys(v, &["protocol", "ok", "error", "data"])
        || v["protocol"] != 1
        || !v["ok"].is_boolean()
        || !(v["error"].is_null() || super::identifier(&v["error"]))
        || (v["ok"] == true) != v["error"].is_null()
        || !keys(
            d,
            &[
                "format_version",
                "action",
                "runner",
                "principal",
                "requester",
                "request_id",
                "target",
                "plan",
                "plan_sha256",
                "approval_id",
                "state",
                "effect_outcome",
                "remote_error",
            ],
        )
        || d["format_version"] != 1
        || d["action"] != method
        || !runner(&d["runner"])
        || d["runner"]["id"] != expected["runner_id"]
        || d["runner"]["server_certificate_sha256"] != expected["server_certificate_sha256"]
        || !keys(&d["principal"], &["id", "client_certificate_sha256"])
        || !d["principal"]["id"]
            .as_str()
            .is_some_and(crate::request::valid_operation)
        || d["principal"]["client_certificate_sha256"] != expected["client_certificate_sha256"]
        || d["request_id"] != request.inputs["request_id"]
        || !nullable_id(&d["requester"])
        || !nullable_id(&d["target"])
        || !nullable_id(&d["approval_id"])
        || !nullable_pin(&d["plan_sha256"])
        || d["remote_error"] != v["error"]
        || !d["state"].as_str().is_some_and(|s| {
            matches!(
                s,
                "not_found"
                    | "planned"
                    | "approved"
                    | "dispatching"
                    | "verified"
                    | "not_applied"
                    | "unknown"
                    | "expired"
            )
        })
        || !d["effect_outcome"]
            .as_str()
            .is_some_and(|s| matches!(s, "not_attempted" | "verified" | "unknown"))
        || (d["state"] == "verified") != (d["effect_outcome"] == "verified")
        || (matches!(d["state"].as_str(), Some("unknown" | "dispatching"))
            && d["effect_outcome"] != "unknown")
    {
        return false;
    }
    if d["plan"].is_null() {
        return d["plan_sha256"].is_null()
            && d["approval_id"].is_null()
            && d["state"] == "not_found"
            && d["effect_outcome"] == "not_attempted";
    }
    let plan = &d["plan"];
    if !valid_plan(plan)
        || d["plan_sha256"] != hash(&serde_json::to_vec(plan).unwrap())
        || plan["request_id"] != d["request_id"]
        || plan["requester"] != d["requester"]
        || plan["target"]["id"] != d["target"]
        || plan["runner"]["id"] != expected["runner_id"]
        || plan["runner"]["server_certificate_sha256"] != expected["server_certificate_sha256"]
        || d["state"] == "not_found"
    {
        return false;
    }
    if let Some(requester) = request.inputs.get("requester") {
        if requester != &d["requester"] {
            return false;
        }
    } else if d["requester"] != d["principal"]["id"] {
        return false;
    }
    if v["ok"] == true
        && (request
            .inputs
            .get("plan_sha256")
            .is_some_and(|p| p != &d["plan_sha256"])
            || request
                .inputs
                .get("approval_id")
                .is_some_and(|p| p != &d["approval_id"])
            || (method == "plan"
                && (request.inputs["target"] != plan["target"]["id"]
                    || request.inputs["proposed_base64"] != plan["proposed_base64"]
                    || request.inputs["expires_in_seconds"] != plan["expires_in_seconds"])))
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lost_write_reply_is_unknown_and_keeps_original_recovery_identity() {
        let mut request = Request::new("change.apply", json!({"request_id":"original"}));
        request.record = "always".into();
        let mut result = OperationResult::new(&request);
        result.data = initial(&request);
        uncertain(&mut result);
        assert_eq!(result.execution.status, Status::Unknown);
        assert_eq!(result.data["request_id"], "original");
        assert_eq!(result.effect_outcome, "unknown");
    }
    #[test]
    fn incomplete_or_unfinished_validation_cannot_authorize_a_plan() {
        let plan: Value = serde_json::from_str(include_str!(
            "../../../../docs/sre-workbench/examples/change-plan.json"
        ))
        .unwrap();
        assert!(valid_plan(&plan));
        for field in ["coverage", "finished_at", "execution", "errors", "output"] {
            let mut changed = plan.clone();
            changed["validation"].as_object_mut().unwrap().remove(field);
            assert!(!valid_plan(&changed), "missing {field}");
        }
        let mut changed = plan.clone();
        changed["validation"]["coverage"]["state"] = json!("partial");
        assert!(!valid_plan(&changed));
        changed = plan;
        changed["validation"]["finished_at"] = json!("2000-01-01T00:00:00Z");
        assert!(!valid_plan(&changed));
    }
    #[test]
    fn fabricated_verified_response_without_a_plan_is_refused() {
        let request = Request::new(
            "change.status",
            json!({"requester":"alice","request_id":"r"}),
        );
        let expected = json!({"runner_id":"runner","server_certificate_sha256":hash(b"server"),"client_certificate_sha256":hash(b"alice")});
        let mut d = initial(&request);
        d["runner"] = json!({"id":"runner","server_certificate_sha256":hash(b"server"),"policy_sha256":hash(b"policy")});
        d["principal"] = json!({"id":"alice","client_certificate_sha256":hash(b"alice")});
        let mut v = json!({"protocol":1,"ok":true,"error":null,"data":d});
        assert!(valid_response(&v, &request, &expected));
        v["data"]["state"] = json!("verified");
        v["data"]["effect_outcome"] = json!("verified");
        assert!(!valid_response(&v, &request, &expected));
        v["data"]["state"] = json!("not_found");
        v["data"]["effect_outcome"] = json!("not_attempted");
        v["data"]["principal"]["client_certificate_sha256"] = json!(hash(b"mallory"));
        assert!(!valid_response(&v, &request, &expected));
    }
}
