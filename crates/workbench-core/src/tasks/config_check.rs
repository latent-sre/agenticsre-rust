//! A declared configuration validator over the existing fixed dashboard checker.
use super::{
    ChildReply, ID, RULES, model_error, parse_reply, run_fixed_python_arguments, source,
    source_identity, valid_digest,
};
use crate::{
    OperatorContext, RunControl,
    request::{Problem, Request, bounded},
    result::{OperationResult, Status},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

const UNSUPPORTED: [&str; 4] = [
    "live_datasource_queries",
    "grafana_rendering",
    "plugin_specific_options",
    "uncovered_dashboard_properties",
];
const SCOPE: &str = "Implemented static textual rules for the consumed Grafana dashboard only; no rendering, live query or deployment-health validation.";
fn bad() -> Problem {
    Problem::invalid(
        "invalid_config_input",
        "Supply only declared kind, an installed validator object, an absolute model path and optional lowercase sha256: pins; nulls and positional arrays are not accepted.",
    )
}
fn input(v: &Value) -> Result<(), Problem> {
    let object = v.as_object().ok_or_else(bad)?;
    if !["kind", "validator", "file"]
        .iter()
        .all(|k| object.contains_key(*k))
        || object.keys().any(|k| {
            !matches!(
                k.as_str(),
                "kind" | "validator" | "file" | "expected_input_sha256"
            )
        })
        || object.values().any(Value::is_null)
    {
        return Err(bad());
    }
    let validator = v["validator"].as_object().ok_or_else(bad)?;
    if !["id", "version"].iter().all(|k| validator.contains_key(*k))
        || validator
            .keys()
            .any(|k| !matches!(k.as_str(), "id" | "version" | "expected_binding_sha256"))
        || validator.values().any(Value::is_null)
    {
        return Err(bad());
    }
    if !v["file"].as_str().is_some_and(|file| {
        bounded(file, 1, 1024)
            && Path::new(file).is_absolute()
            && !file.bytes().any(|byte| byte.is_ascii_control())
    }) {
        return Err(bad());
    }
    for pin in [
        object.get("expected_input_sha256"),
        validator.get("expected_binding_sha256"),
    ]
    .into_iter()
    .flatten()
    {
        if !pin.as_str().is_some_and(valid_digest) {
            return Err(bad());
        }
    }
    if !v["kind"].is_string()
        || !v["validator"]["id"].is_string()
        || !v["validator"]["version"].is_u64()
    {
        return Err(bad());
    }
    if !matches!(
        v["kind"].as_str(),
        Some("grafana-dashboard" | "prometheus-rules")
    ) {
        return Err(Problem::unsupported(
            "unsupported_config_kind",
            "Only declared grafana-dashboard and prometheus-rules kinds are supported by installed bindings.",
        ));
    }
    let expected = if v["kind"] == "grafana-dashboard" {
        ID
    } else {
        "promtool"
    };
    if v["validator"]["id"] != expected || v["validator"]["version"] != 1 {
        return Err(Problem::unsupported(
            "unsupported_validator",
            "Select the installed version-1 validator matching the declared configuration kind.",
        ));
    }
    Ok(())
}
fn envelope(request: &Request) -> Value {
    let identity = source_identity();
    json!({"kind":"grafana-dashboard","validator":{"id":ID,"version":1,"checker_digest":identity["checker_digest"],"binding_digest":identity["binding_digest"],"upstream_revision":identity["upstream_revision"]},
        "input":{"file":request.inputs["file"],"digest":null,"bytes":null},"implemented_rules":RULES,"unsupported_sections":UNSUPPORTED,
        "validation_state":"not_established","checked_panels":0,"findings":[],"findings_total":0,"findings_truncated":false,"fields_truncated":false,"runtime":null,"cancellation_signal":null})
}
fn controlled(control: &RunControl, deadline: Instant, result: &mut OperationResult) -> bool {
    let signal = control.signal.load(Ordering::Relaxed);
    let code = if signal != 0 {
        Some("cancelled")
    } else if Instant::now() >= deadline {
        Some("deadline_exceeded")
    } else {
        None
    };
    if let Some(code) = code {
        result.execution.status = if signal != 0 {
            Status::Cancelled
        } else {
            Status::TimedOut
        };
        result.data["validation_state"] = json!("not_established");
        result.data["cancellation_signal"] = if signal == 0 {
            Value::Null
        } else {
            json!(signal)
        };
        result.coverage.state = "partial".into();
        result.error(code,"Configuration validation stopped within its invocation budget; no complete validation is claimed.");
        true
    } else {
        false
    }
}
// Tighten error metadata for this operation without changing the legacy task protocol.
fn reply(payload: &str, pin: Option<&str>) -> Option<ChildReply> {
    let reply = parse_reply(payload)?;
    if reply.status == "error" {
        if reply.checked_panels != 0
            || reply.findings_total != 0
            || reply.findings_truncated
            || reply.fields_truncated
        {
            return None;
        }
        let no_read = matches!(
            reply.error_code.as_deref(),
            Some(
                "unsupported_runtime"
                    | "invalid_task_input"
                    | "non_regular_model"
                    | "model_unreadable"
                    | "model_too_large"
            )
        );
        if no_read && (reply.input_digest.is_some() || reply.input_bytes != 0) {
            return None;
        }
        if !no_read
            && reply.error_code.as_deref() != Some("task_internal_error")
            && reply.input_digest.is_none()
        {
            return None;
        }
    }
    match (
        pin,
        reply.input_digest.as_deref(),
        reply.error_code.as_deref(),
    ) {
        (Some(expected), Some(actual), Some("input_digest_mismatch")) if expected != actual => {}
        (_, _, Some("input_digest_mismatch")) => return None,
        (Some(expected), Some(actual), _) if expected != actual => return None,
        _ => {}
    }
    Some(reply)
}
pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    if let Err(problem) = input(&request.inputs) {
        result.reject(problem);
        return;
    }
    if request.inputs["kind"] == "prometheus-rules" {
        super::prometheus_rules::run(request, control, context, start, result);
        return;
    }
    result.data = envelope(request);
    result.coverage.scope = SCOPE.into();
    result.coverage.limitations=vec!["Only the ten installed textual hygiene rules are implemented; uncovered dashboard properties and plugin-specific options are not validated.".into(),"Findings do not establish Grafana rendering, query correctness, deployed configuration or live service health. Source bytes are untrusted data and are never executed.".into()];
    if request.inputs["validator"]
        .get("expected_binding_sha256")
        .is_some_and(|pin| pin != &result.data["validator"]["binding_digest"])
    {
        result.reject(Problem::invalid("validator_digest_mismatch","The selected installed binding does not match the expected validator digest; no model was read and no child was started."));
        return;
    }
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    if controlled(control, deadline, result) {
        return;
    }
    let mut args = vec![
        request.inputs["file"]
            .as_str()
            .expect("validated path")
            .into(),
    ];
    args.push("--config-check-v1".into());
    let pin = request
        .inputs
        .get("expected_input_sha256")
        .and_then(Value::as_str);
    if let Some(pin) = pin {
        args.push(pin.into());
    }
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .min(request.limits.timeout_ms as u128) as u64;
    if remaining == 0 {
        result.execution.status = Status::TimedOut;
        result.coverage.state = "partial".into();
        result.error(
            "deadline_exceeded",
            "Insufficient invocation budget remains to start the validator.",
        );
        return;
    }
    result.effective_limits.timeout_ms = remaining;
    let invocation =
        run_fixed_python_arguments(ID, source(), source_identity(), args, control, result);
    result.coverage.scope = SCOPE.into();
    let task_data = std::mem::replace(&mut result.data, envelope(request));
    if let Some(supervisor) = task_data.get("supervisor") {
        result.data["supervisor"] = supervisor.clone();
    }
    if let Some(signal) = task_data.get("cancellation_signal") {
        result.data["cancellation_signal"] = signal.clone();
    }
    let Some((executable, payload)) = invocation else {
        result.data["validation_state"] = json!(match result.execution.status {
            Status::Partial => "partial",
            Status::Unsupported => "unsupported",
            _ => "not_established",
        });
        result.data["findings_truncated"] = json!(result.output.stdout_truncated);
        if result.coverage.state == "not_applicable" {
            result.coverage.state = "not_established".into();
        }
        return;
    };
    let Some(reply) = reply(&payload, pin) else {
        super::invalid_response(result);
        return;
    };
    result.data["runtime"] = json!({"executable":executable,"version":reply.runtime_version});
    if let Some(input_digest) = &reply.input_digest {
        result.data["input"]["digest"] = json!(input_digest);
        result.data["input"]["bytes"] = json!(reply.input_bytes);
        result.sources = vec![
            json!({"reference":format!("file:{}@{}",request.inputs["file"].as_str().unwrap(),input_digest),"observed_at":result.started_at,"evidence_state":"sourced","untrusted":true}),
        ];
    }
    if let Some(code) = &reply.error_code {
        let unsupported = matches!(
            code.as_str(),
            "unsupported_model_v2" | "unsupported_model_schema" | "unsupported_runtime"
        );
        result.execution.status = if code == "unsupported_runtime" {
            Status::Unsupported
        } else {
            Status::Failed
        };
        result.data["validation_state"] = json!(if unsupported {
            "unsupported"
        } else {
            "not_established"
        });
        result.coverage.state = "not_established".into();
        if code == "unsupported_model_v2" {
            result.data["unsupported_sections"]
                .as_array_mut()
                .unwrap()
                .push(json!("grafana_dashboard_v2"));
        }
        if code == "unsupported_model_schema" {
            result.data["unsupported_sections"]
                .as_array_mut()
                .unwrap()
                .push(json!("grafana_dashboard_schema"));
        }
        result.error(code, model_error(code).expect("validated error"));
    } else {
        result.data["checked_panels"] = json!(reply.checked_panels);
        result.data["findings"] = json!(reply.findings);
        result.data["findings_total"] = json!(reply.findings_total);
        result.data["findings_truncated"] = json!(reply.findings_truncated);
        result.data["fields_truncated"] = json!(reply.fields_truncated);
        let partial = reply.status == "partial";
        result.execution.status = if partial {
            Status::Partial
        } else {
            Status::Succeeded
        };
        result.data["validation_state"] = json!(if partial { "partial" } else { "complete" });
        result.coverage.state = if partial { "partial" } else { "complete" }.into();
        if partial {
            result.error("config_findings_truncated","Finding fields or the finding list were shortened; validation output is incomplete.");
        }
    }
    controlled(control, deadline, result);
}

#[cfg(test)]
mod tests {
    use super::super::digest;
    use super::*;
    fn valid() -> Value {
        json!({"kind":"grafana-dashboard","validator":{"id":ID,"version":1},"file":"/tmp/config.json"})
    }
    #[test]
    fn strict_input_rejects_nested_arrays_nulls_unknown_fields_and_bad_pins() {
        assert!(input(&valid()).is_ok());
        for (key, value) in [
            ("validator", json!([ID, 1])),
            (
                "validator",
                json!({"id":ID,"version":1,"expected_binding_sha256":null}),
            ),
            (
                "validator",
                json!({"id":ID,"version":1,"command":"echo injected"}),
            ),
            ("expected_input_sha256", Value::Null),
            ("expected_input_sha256", json!("sha256:ABC")),
            ("file", json!("relative.json")),
            ("file", json!("/tmp/model\n.json")),
            ("file", json!("/tmp/model\t.json")),
            ("file", json!("/tmp/model\u{007f}.json")),
            ("command", json!("echo injected")),
        ] {
            let mut v = valid();
            v[key] = value;
            assert!(input(&v).is_err(), "{key}");
        }
        assert!(input(&json!(["grafana-dashboard",{"id":ID,"version":1},"/tmp/x"])).is_err());
    }
    #[test]
    fn expired_budget_never_spawns_or_emits_zero_effective_limit_or_signal() {
        let mut request = Request::new("config.check", valid());
        request.limits.timeout_ms = 1;
        let mut result = OperationResult::new(&request);
        run(
            &request,
            &RunControl::default(),
            &OperatorContext::default(),
            Instant::now() - Duration::from_millis(2),
            &mut result,
        );
        assert_eq!(result.execution.status, Status::TimedOut);
        assert_eq!(result.effect_outcome, "not_attempted");
        assert!(result.effective_limits.timeout_ms >= 1);
        assert!(result.data["cancellation_signal"].is_null());
        assert!(result.data["supervisor"].is_null());
        assert!(result.data["input"]["digest"].is_null());
    }

    #[test]
    fn reply_refuses_inconsistent_pins_and_fabricated_error_counts() {
        let actual = digest(b"{}");
        let other = digest(b"different");
        let mut value = json!({"task":{"id":ID,"version":1},"status":"complete","runtime_version":"3.11.2","input_digest":actual,"input_bytes":2,"checked_panels":1,"findings":[],"findings_total":0,"findings_truncated":false,"fields_truncated":false,"error_code":null});
        assert!(reply(&value.to_string(), Some(&actual)).is_some());
        assert!(reply(&value.to_string(), Some(&other)).is_none());
        value["status"] = json!("error");
        value["checked_panels"] = json!(0);
        value["error_code"] = json!("input_digest_mismatch");
        assert!(reply(&value.to_string(), Some(&other)).is_some());
        assert!(reply(&value.to_string(), Some(&actual)).is_none());
        assert!(reply(&value.to_string(), None).is_none());
        value["findings_total"] = json!(1);
        assert!(reply(&value.to_string(), Some(&other)).is_none());
        value["findings_total"] = json!(0);
        value["error_code"] = json!("model_unreadable");
        assert!(reply(&value.to_string(), None).is_none());
    }
}
