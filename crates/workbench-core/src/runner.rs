//! Authenticated, explicitly started remote execution. Transport identity is never a wire grant.
#[cfg(target_os = "linux")]
mod config;
#[cfg(target_os = "linux")]
mod response;
#[cfg(target_os = "linux")]
mod server;
#[cfg(target_os = "linux")]
mod transport;
use crate::{
    OperatorContext, RunControl,
    request::Request,
    result::{OperationResult, Status},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};
#[derive(Debug)]
pub struct RunnerError {
    pub code: &'static str,
    pub effects: bool,
}
impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Remote runner refused or could not establish the operation ({})",
            self.code
        )
    }
}
impl std::error::Error for RunnerError {}
impl From<&'static str> for RunnerError {
    fn from(code: &'static str) -> Self {
        Self {
            code,
            effects: false,
        }
    }
}
fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "sha256:{}",
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
fn keys(v: &Value, required: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|o| o.len() == required.len() && required.iter().all(|k| o.contains_key(*k)))
}
fn text(v: &str, max: usize) -> bool {
    !v.is_empty() && v.len() <= max && !v.chars().any(char::is_control)
}
fn pin(v: &str) -> bool {
    v.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn active(control: &RunControl, deadline: Instant) -> Result<(), RunnerError> {
    if control.signal.load(std::sync::atomic::Ordering::Relaxed) != 0 {
        Err("runner_cancelled".into())
    } else if Instant::now() >= deadline {
        Err("runner_deadline_exceeded".into())
    } else {
        Ok(())
    }
}
fn input(method: &str, v: &Value) -> bool {
    match method {
        "handshake" => keys(v, &[]),
        "status" | "cancel" => {
            keys(v, &["request_id"])
                && v["request_id"]
                    .as_str()
                    .is_some_and(|s| crate::request::bounded(s, 1, 128))
        }
        "watch" => {
            keys(v, &["request_id", "after_sequence"])
                && v["request_id"]
                    .as_str()
                    .is_some_and(|s| crate::request::bounded(s, 1, 128))
                && v["after_sequence"]
                    .as_u64()
                    .is_some_and(|n| n <= 9_007_199_254_740_991)
        }
        "submit" => {
            keys(
                v,
                &[
                    "request_id",
                    "target",
                    "operation",
                    "operation_version",
                    "inputs",
                    "max_runtime_ms",
                    "retain_events",
                ],
            ) && v["request_id"]
                .as_str()
                .is_some_and(|s| crate::request::bounded(s, 1, 128))
                && v["target"]
                    .as_str()
                    .is_some_and(crate::request::valid_operation)
                && v["operation"]
                    .as_str()
                    .is_some_and(|s| s == "task.run" || s.starts_with("ext."))
                && v["operation_version"] == 1
                && v["inputs"].is_object()
                && v["max_runtime_ms"]
                    .as_u64()
                    .is_some_and(|n| (1..=3_600_000).contains(&n))
                && v["retain_events"]
                    .as_u64()
                    .is_some_and(|n| (1..=256).contains(&n))
        }
        _ => false,
    }
}
pub fn serve_runner(
    path: &Path,
    control: &RunControl,
    ready: impl FnOnce(Value) -> Result<(), ()>,
) -> Result<(), RunnerError> {
    #[cfg(target_os = "linux")]
    {
        server::serve(path, control, ready)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, control, ready);
        Err("runner_platform_unsupported".into())
    }
}
pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    result.data = initial(request);
    result.record_mode = if request.record == "always" {
        "always"
    } else if request.record == "auto" {
        "auto"
    } else {
        "never"
    };
    result.coverage.scope="Authenticated remote lifecycle observations; target metadata and caller-supplied task inputs do not establish current service health.".into();
    #[cfg(target_os = "linux")]
    let outcome = client(request, control, context, start, result);
    #[cfg(not(target_os = "linux"))]
    let outcome: Result<(), RunnerError> = Err("runner_platform_unsupported".into());
    let signal = control.signal.load(std::sync::atomic::Ordering::Relaxed);
    if signal != 0 {
        result.data["cancellation_signal"] = json!(signal);
    }
    if let Err(error) = outcome {
        result.execution.status = match error.code {
            "runner_cancelled" => Status::Cancelled,
            "runner_deadline_exceeded" => Status::TimedOut,
            "runner_admission_unknown" => Status::Unknown,
            _ => Status::Denied,
        };
        result.error(error.code,"Remote action could not be established. Query the same request identity to reconcile uncertain admission; no automatic resubmission is performed.");
    }
}
#[cfg(target_os = "linux")]
fn client(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) -> Result<(), RunnerError> {
    let method = request.operation.strip_prefix("runner.").unwrap_or("");
    if !input(method, &request.inputs) {
        return Err("invalid_runner_input".into());
    }
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    active(control, deadline)?;
    let prepared = prepare_client(
        context
            .runner_config_path
            .as_deref()
            .ok_or(RunnerError::from("runner_config_required"))?,
        control,
        deadline,
    )?;
    client_prepared(request, control, &prepared, None, deadline, result)
}
#[cfg(target_os = "linux")]
fn client_prepared(
    request: &Request,
    control: &RunControl,
    prepared: &PreparedClient,
    expected: Option<&Value>,
    deadline: Instant,
    result: &mut OperationResult,
) -> Result<(), RunnerError> {
    let method = request.operation.trim_start_matches("runner.");
    if !input(method, &request.inputs) || expected.is_some_and(|e| !valid_expected(e)) {
        return Err("invalid_runner_input".into());
    }
    active(control, deadline)?;
    let cfg = &prepared.config;
    if cfg.runner_id != request.target.id {
        return Err("runner_identity_mismatch".into());
    }
    let client_certificate_sha256 = &prepared.identity["client_certificate_sha256"];
    let tls = prepared.tls.clone();
    let mut wire_value = json!({"protocol":1,"method":method,"inputs":request.inputs});
    if let Some(expected) = expected {
        wire_value["expected"] = expected.clone();
    }
    let wire = serde_json::to_vec(&wire_value).unwrap();
    if wire.len() > 65536 {
        return Err("runner_request_too_large".into());
    }
    result.data["runner"] = json!({"id":cfg.runner_id,"protocol":1,"platform":null,"server_certificate_sha256":cfg.server_certificate_sha256,"policy_sha256":null});
    let bytes = match transport::exchange_controlled(
        cfg.endpoint
            .parse()
            .map_err(|_| RunnerError::from("invalid_runner_config"))?,
        &cfg.server_name,
        &cfg.server_certificate_sha256,
        tls,
        &wire,
        deadline,
        Some(control.signal.clone()),
    ) {
        Ok(v) => v,
        Err(e) => {
            if matches!(method, "submit" | "cancel") && e.request_may_have_been_sent {
                result.data["admission"] = json!("unknown");
                result.effect_outcome = "unknown".into();
                result.data["management_effect_outcome"] = json!("unknown");
                return Err("runner_admission_unknown".into());
            }
            return Err(e.code.into());
        }
    };
    let response = match crate::request::parse_bounded_json(&bytes, 48) {
        Ok(v) => v,
        Err(_) => return uncertain(method, result, "runner_invalid_response"),
    };
    if !response::valid(&response)
        || !keys(&response, &["protocol", "ok", "error", "data"])
        || response["protocol"] != 1
        || !response["ok"].is_boolean()
        || !response["data"].is_object()
        || response["data"]["runner"]["id"] != cfg.runner_id
        || response["data"]["runner"]["server_certificate_sha256"] != cfg.server_certificate_sha256
        || &response["data"]["principal"]["client_certificate_sha256"] != client_certificate_sha256
        || response["data"]["request_id"]
            != request
                .inputs
                .get("request_id")
                .cloned()
                .unwrap_or(Value::Null)
        || (method == "submit"
            && !response["data"]["binding"].is_null()
            && (response["data"]["binding"]["submission_sha256"]
                != hash(&serde_json::to_vec(&request.inputs).unwrap())
                || response["data"]["target"]["id"] != request.inputs["target"]))
        || bytes.len() > 1536 * 1024
    {
        return uncertain(method, result, "runner_invalid_response");
    }
    if response["ok"] == true
        && expected.is_some_and(|e| {
            response["data"]["principal"]["id"] != e["principal"]
                || e.get("policy_sha256")
                    .is_some_and(|p| p != &response["data"]["runner"]["policy_sha256"])
        })
    {
        return uncertain(method, result, "runner_precondition_failed");
    }
    result.data = response["data"].clone();
    if response["ok"] != true {
        result.execution.status = match response["data"]["management_effect_outcome"].as_str() {
            Some("not_attempted") => Status::Denied,
            Some("unknown") => Status::Unknown,
            _ => Status::Failed,
        };
        result.error(
            "runner_remote_refusal",
            "The authenticated runner refused this action; inspect the bounded remote_error field.",
        );
        result.data["remote_error"] = response["error"].clone();
    }
    result.effect_outcome = response["data"]["management_effect_outcome"]
        .as_str()
        .unwrap_or("unknown")
        .into();
    let budget = result.effective_limits.max_output_bytes;
    let detail_size = |data: &Value| {
        serde_json::to_vec(&json!({"result":data["result"],"events":data["events"]}))
            .unwrap()
            .len()
    };
    let mut omitted = false;
    if detail_size(&result.data) > budget && !result.data["result"].is_null() {
        result.data["result"] = Value::Null;
        result.data["result_omitted"] = json!(true);
        omitted = true;
    }
    while detail_size(&result.data) > budget {
        let Some(events) = result.data["events"].as_array_mut() else {
            break;
        };
        if events.pop().is_none() {
            break;
        }
        omitted = true;
    }
    if omitted {
        let next = result.data["events"]
            .as_array()
            .and_then(|a| a.last())
            .map(|e| e["sequence"].clone())
            .unwrap_or_else(|| result.data["cursor"]["after_sequence"].clone());
        if result.data["cursor"].is_object() {
            result.data["cursor"]["next_sequence"] = next.clone();
            result.data["cursor"]["has_more"] =
                json!(next.as_u64() < result.data["cursor"]["last_sequence"].as_u64());
        }
        if result.execution.status == Status::Succeeded {
            result.execution.status = Status::Partial;
        }
        result.coverage.state = "partial".into();
    }
    if let Err(e) = active(control, deadline) {
        if matches!(method, "submit" | "cancel") {
            return uncertain(method, result, e.code);
        }
        return Err(e);
    }
    Ok(())
}
#[cfg(target_os = "linux")]
pub(crate) fn check_grant(
    request: &Request,
    implementation: &Value,
    scope: &Value,
) -> Result<(), &'static str> {
    let grant = &scope["grant"];
    if grant["operation"] != request.operation
        || grant["operation_version"] != request.operation_version
    {
        return Err("runner_grant_denied");
    }
    if request.operation == "task.run" {
        if request.inputs["id"] != "error-budget"
            || request.inputs["version"] != 1
            || grant["task"]["id"] != request.inputs["id"]
            || grant["task"]["version"] != request.inputs["version"]
            || grant["task"]["binding_sha256"] != implementation["source"]["binding_digest"]
        {
            return Err("runner_grant_denied");
        }
    } else if !request.operation.starts_with("ext.")
        || grant["package_sha256"] != implementation["package_sha256"]
        || grant["policy_sha256"] != implementation["policy_sha256"]
    {
        return Err("runner_grant_denied");
    }
    Ok(())
}

fn initial(request: &Request) -> Value {
    json!({"format_version":1,"action":request.operation.trim_start_matches("runner."),"admission":"not_requested","runner":null,"principal":null,"target":null,"request_id":request.inputs.get("request_id"),"request_sha256":null,"job":null,"events":[],"cursor":null,"result":null,"result_sha256":null,"result_omitted":false,"clock":null,"cancellation_signal":null,"capabilities":[],"limits":{"max_request_bytes":65536,"max_response_bytes":2097152,"max_runtime_ms":3600000,"max_workers":2,"max_connections":8},"remote_error":null,"admission_expires_at":null,"management_effect_outcome":"not_attempted","binding":null})
}
#[cfg(target_os = "linux")]
fn uncertain(
    method: &str,
    result: &mut OperationResult,
    code: &'static str,
) -> Result<(), RunnerError> {
    if matches!(method, "submit" | "cancel") {
        result.data["admission"] = json!("unknown");
        result.effect_outcome = "unknown".into();
        result.data["management_effect_outcome"] = json!("unknown");
        Err("runner_admission_unknown".into())
    } else {
        Err(code.into())
    }
}
#[cfg(target_os = "linux")]
pub(crate) struct PreparedClient {
    config: config::Client,
    tls: std::sync::Arc<rustls::ClientConfig>,
    pub identity: Value,
}
#[cfg(target_os = "linux")]
pub(crate) fn prepare_client(
    path: &Path,
    control: &RunControl,
    deadline: Instant,
) -> Result<PreparedClient, RunnerError> {
    let (config, profile_sha256) = config::client(path, control, deadline)?;
    let material = config.material(control, deadline)?;
    let identity = json!({"profile_reference":path,"profile_sha256":profile_sha256,"endpoint":config.endpoint,"server_name":config.server_name,"runner_id":config.runner_id,"server_certificate_sha256":config.server_certificate_sha256,"client_certificate_sha256":hash(&material.certificate),"ca_certificate_sha256":hash(&material.ca)});
    let tls = transport::client_config(material).map_err(RunnerError::from)?;
    Ok(PreparedClient {
        config,
        tls,
        identity,
    })
}
/// Reuse the pinned mTLS channel without granting change authority from the caller.
#[cfg(target_os = "linux")]
pub(crate) fn exchange_change(
    prepared: &PreparedClient,
    wire: &[u8],
    control: &RunControl,
    deadline: Instant,
) -> Result<Vec<u8>, (&'static str, bool)> {
    if wire.len() > 65536 {
        return Err(("runner_request_too_large", false));
    }
    transport::exchange_controlled(
        prepared
            .config
            .endpoint
            .parse()
            .map_err(|_| ("invalid_runner_config", false))?,
        &prepared.config.server_name,
        &prepared.config.server_certificate_sha256,
        prepared.tls.clone(),
        wire,
        deadline,
        Some(control.signal.clone()),
    )
    .map_err(|e| (e.code, e.request_may_have_been_sent))
}
#[cfg(target_os = "linux")]
pub(crate) fn read_private(
    path: &Path,
    max: usize,
    c: &RunControl,
    d: Instant,
) -> Result<Vec<u8>, RunnerError> {
    config::read(path, max, c, d)
}
#[cfg(target_os = "linux")]
pub(crate) fn prepared_call(
    prepared: &PreparedClient,
    method: &str,
    inputs: Value,
    expected: &Value,
    control: &RunControl,
    deadline: Instant,
) -> OperationResult {
    let mut request = Request::new(&format!("runner.{method}"), inputs);
    request.target.kind = "runner".into();
    request.target.id = prepared.config.runner_id.clone();
    if matches!(method, "submit" | "cancel") {
        request.record = "always".into();
    }
    request.limits.max_output_bytes = 64 * 1024;
    request.limits.timeout_ms = deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .clamp(1, 300_000) as u64;
    let start = Instant::now();
    let mut result = OperationResult::new(&request);
    result.data = initial(&request);
    result.record_mode = if request.record == "always" {
        "always"
    } else {
        "never"
    };
    if let Err(error) = client_prepared(
        &request,
        control,
        prepared,
        Some(expected),
        deadline,
        &mut result,
    ) {
        result.execution.status = match error.code {
            "runner_cancelled" => Status::Cancelled,
            "runner_deadline_exceeded" => Status::TimedOut,
            "runner_admission_unknown" => Status::Unknown,
            _ => Status::Denied,
        };
        result.error(error.code,"The pinned remote observation could not be established; no retry admission is authorized.");
    }
    result.finish(start);
    result
}
fn valid_expected(v: &Value) -> bool {
    (keys(v, &["principal"]) || keys(v, &["principal", "policy_sha256"]))
        && v["principal"]
            .as_str()
            .is_some_and(crate::request::valid_operation)
        && v.get("policy_sha256")
            .is_none_or(|p| p.as_str().is_some_and(pin))
}
#[cfg(target_os = "linux")]
pub(crate) fn valid_budget_data(value: &Value) -> bool {
    response::budget_data(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncertain_writes_never_become_safe_retries() {
        for method in ["submit", "cancel"] {
            let request = Request::new(&format!("runner.{method}"), json!({"request_id":"known"}));
            let mut result = OperationResult::new(&request);
            result.data = initial(&request);
            let e = uncertain(method, &mut result, "runner_invalid_response").unwrap_err();
            assert_eq!(e.code, "runner_admission_unknown");
            assert_eq!(result.data["request_id"], "known");
            assert_eq!(result.data["admission"], "unknown");
            assert_eq!(result.effect_outcome, "unknown");
        }
    }
    #[test]
    fn invalid_protocol_input_has_no_command_or_identity_authority() {
        assert!(!input(
            "submit",
            &json!({"request_id":"r","target":"t","operation":"process.exec","operation_version":1,"inputs":{},"max_runtime_ms":1000,"retain_events":2})
        ));
        assert!(!input(
            "status",
            &json!({"request_id":"r","principal":"administrator"})
        ));
    }
}
