//! Two compiled-in offline tasks. There is no request-controlled registration or interpreter.
use crate::{
    RunControl, process,
    request::{Problem, Request, bounded},
    result::{OperationResult, Status},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::OnceLock;

mod error_budget;

/// Validate and normalize only the installed numerical task schema, without spawning work.
pub fn normalize_error_budget_input(input: &Value) -> Result<Value, Problem> {
    error_budget::normalize_input(input)
}

const ID: &str = "dashboard-hygiene";
const REVISION: &str = "7d900fb999abee3b3712e1da881dcc7e80b1f6e7";
const CHECKER: &str = include_str!("../resources/dashboard-hygiene/dashboard_hygiene.py");
const WRAPPER: &str = include_str!("../resources/dashboard-hygiene/wrapper.py");
const MAX_CHILD_RESPONSE: usize = 512 * 1024;

fn digest(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut value = String::from("sha256:");
    for byte in Sha256::digest(bytes) {
        write!(value, "{byte:02x}").expect("string writing");
    }
    value
}

fn source() -> &'static str {
    static SOURCE: OnceLock<String> = OnceLock::new();
    SOURCE.get_or_init(|| {
        WRAPPER.replace(
            "__WORKBENCH_CHECKER_SOURCE__",
            &serde_json::to_string(CHECKER).expect("constant source"),
        )
    })
}

fn source_identity() -> Value {
    json!({"upstream_revision":REVISION,"checker_digest":digest(CHECKER.as_bytes()),"binding_digest":digest(source().as_bytes())})
}

fn python() -> Result<process::PreparedProcess, Problem> {
    if !cfg!(target_os = "linux") {
        return Err(Problem::unsupported(
            "unsupported_platform",
            "Fixed task execution is currently implemented only on Linux.",
        ));
    }
    process::prepare(&json!({"program":"python3","args":[],"cwd":"/"})).map_err(|_| {
        Problem::unsupported("missing_dependency", "A native Python 3 installation is required in /usr/bin or /bin; no interpreter was started.")
    })
}

pub fn availability() -> Value {
    match python() {
        Ok(prepared) => {
            json!({"status":"available","reason":"Python is present; version >=3.11 is checked inside the supervised invocation, not probed by discovery.","runtime_executable":prepared.executable,"version_verified":false})
        }
        Err(problem) => {
            json!({"status":if problem.code == "missing_dependency" { "missing_dependency" } else { "unsupported_platform" },"reason":problem.message,"runtime_executable":null,"version_verified":false})
        }
    }
}

fn metadata() -> Value {
    json!({"spec_version":"0.1","id":ID,"version":1,"entrypoint":"embedded/dashboard-hygiene/wrapper.py",
        "digest":digest(source().as_bytes()),"runtime":{"kind":"python","version_requirement":">=3.11; checked inside each invocation"},
        "input_schema":{"$ref":"urn:sre-workbench:spec:dashboard-hygiene-input:0.1"},
        "output_schema":{"$ref":"urn:sre-workbench:spec:dashboard-hygiene-data:0.1"},
        "arguments":[{"input":"file"}],"allowed_environment":["PATH","LANG","LC_ALL","TZ"],"effects":"observe","timeout_ms":30000,"platforms":["linux"]})
}

pub(crate) fn list() -> Value {
    json!({"tasks":[{"task":metadata(),"availability":availability()},{"task":error_budget::metadata(),"availability":availability()}]})
}

pub(crate) fn describe(inputs: &Value, result: &mut OperationResult) {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        id: String,
    }
    match serde_json::from_value::<Input>(inputs.clone()) {
        Ok(input) if input.id == ID => {
            result.effect_outcome = "not_applicable".into();
            result.data =
                json!({"task":metadata(),"availability":availability(),"source":source_identity()});
        }
        Ok(input) if input.id == error_budget::ID => {
            result.effect_outcome = "not_applicable".into();
            result.data = json!({"task":error_budget::metadata(),"availability":availability(),"source":error_budget::source_identity()});
        }
        Ok(_) => result.reject(Problem::unsupported(
            "unknown_task",
            "The requested task is not installed.",
        )),
        Err(_) => result.reject(Problem::invalid(
            "invalid_task_input",
            "Task description requires only an id string.",
        )),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskInput {
    id: String,
    version: u64,
    input: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelInput {
    file: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskIdentity {
    id: String,
    version: u64,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Finding {
    rule: String,
    location: String,
    detail: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChildReply {
    task: TaskIdentity,
    status: String,
    runtime_version: String,
    #[serde(deserialize_with = "required_option")]
    input_digest: Option<String>,
    input_bytes: u64,
    checked_panels: u64,
    findings: Vec<Finding>,
    findings_total: u64,
    findings_truncated: bool,
    fields_truncated: bool,
    #[serde(deserialize_with = "required_option")]
    error_code: Option<String>,
}

fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

fn runtime_version_supported(version: &str) -> Option<bool> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit())
        })
    {
        return None;
    }
    Some((parts[0].parse::<u16>().ok()?, parts[1].parse::<u16>().ok()?) >= (3, 11))
}

fn valid_reply(reply: &ChildReply) -> bool {
    let Some(runtime_supported) = runtime_version_supported(&reply.runtime_version) else {
        return false;
    };
    if reply.task.id != ID
        || reply.task.version != 1
        || reply.input_bytes > 2 * 1024 * 1024
        || reply.checked_panels > 1000
        || reply.findings.len() > 1000
        || reply.findings_total > 2 * 1024 * 1024
        || reply.findings_total < reply.findings.len() as u64
        || reply
            .input_digest
            .as_ref()
            .is_some_and(|s| !valid_digest(s))
        || reply.findings.iter().any(|f| {
            !RULES.contains(&f.rule.as_str())
                || !bounded(&f.location, 1, 512)
                || !bounded(&f.detail, 1, 512)
        })
    {
        return false;
    }
    match reply.status.as_str() {
        "error" => {
            reply
                .error_code
                .as_deref()
                .is_some_and(|code| model_error(code).is_some())
                && reply.findings.is_empty()
                && (reply.error_code.as_deref() == Some("unsupported_runtime")) != runtime_supported
        }
        "complete" | "partial" => {
            runtime_supported
                && reply.error_code.is_none()
                && reply.input_digest.is_some()
                && reply.checked_panels > 0
                && (reply.findings_truncated
                    == (reply.findings_total > reply.findings.len() as u64))
                && ((reply.status == "partial")
                    == (reply.findings_truncated || reply.fields_truncated))
        }
        _ => false,
    }
}

fn parse_reply(payload: &str) -> Option<ChildReply> {
    let value: Value = serde_json::from_str(payload).ok()?;
    if !value.is_object()
        || !value["task"].is_object()
        || !value["findings"]
            .as_array()
            .is_some_and(|findings| findings.iter().all(Value::is_object))
    {
        return None;
    }
    // Decode the original bytes so duplicate fields remain errors rather than being
    // normalized away by the shape inspection's Value representation.
    serde_json::from_str::<ChildReply>(payload)
        .ok()
        .filter(valid_reply)
}

const RULES: [&str; 10] = [
    "panel-title",
    "panel-description",
    "panel-units",
    "panel-no-targets",
    "panel-no-value",
    "panel-datasource",
    "target-rate-interval",
    "target-counter-agg",
    "template-all-value",
    "dashboard-tags",
];

fn model_error(code: &str) -> Option<&'static str> {
    Some(match code {
        "unsupported_runtime" => {
            "The selected Python runtime is older than the required version 3.11."
        }
        "invalid_task_input" => "The fixed task received invalid input arguments.",
        "non_regular_model" => {
            "The model must be a regular file; directories and FIFOs are not accepted."
        }
        "model_unreadable" => "The model file could not be opened or read.",
        "model_too_large" => "The model exceeds the 2 MiB input limit.",
        "model_too_deep" => "The model exceeds the 32-container nesting limit.",
        "invalid_model_encoding" => "The model is not valid UTF-8.",
        "invalid_model_json" => "The model is not valid bounded JSON.",
        "duplicate_model_key" => "The model contains duplicate object keys.",
        "invalid_model" => {
            "The model contains missing or incorrectly typed fields required by this checker."
        }
        "unsupported_model_v2" => {
            "V2 dashboard models require a V2-capable checker and are not checked by this task."
        }
        "too_many_panels" => "The model exceeds the 1000 leaf-panel limit.",
        "query_too_large" => "A query exceeds the 16000-character limit.",
        "uncheckable_model" => "The model has no leaf panels that this checker can examine.",
        "task_internal_error" => {
            "The fixed checker could not complete; no input or exception body is disclosed."
        }
        _ => return None,
    })
}

pub(crate) fn run(request: &Request, control: &RunControl, result: &mut OperationResult) {
    let input = match serde_json::from_value::<TaskInput>(request.inputs.clone()) {
        Ok(input) => input,
        Err(_) => {
            result.reject(Problem::invalid(
                "invalid_task_input",
                "Task inputs require only id, version and the selected task's input object.",
            ));
            return;
        }
    };
    if !input.input.is_object() {
        result.reject(Problem::invalid(
            "invalid_task_input",
            "The selected task's input must be a JSON object.",
        ));
        return;
    }
    if input.version == 1 && input.id == error_budget::ID {
        error_budget::run(&input.input, control, result);
        return;
    }
    if input.id != ID || input.version != 1 {
        result.reject(Problem::unsupported(
            "unsupported_task",
            "Only installed dashboard-hygiene and error-budget task version 1 bindings are supported.",
        ));
        return;
    }
    let input = match serde_json::from_value::<ModelInput>(input.input) {
        Ok(input) => input,
        Err(_) => {
            result.reject(Problem::invalid(
                "invalid_task_input",
                "Dashboard-hygiene accepts only input.file.",
            ));
            return;
        }
    };
    if !bounded(&input.file, 1, 1024) || !Path::new(&input.file).is_absolute() {
        result.reject(Problem::invalid("absolute_model_path_required", "Structured task requests require an absolute UTF-8 model path of at most 1024 characters."));
        return;
    }
    let invocation = run_fixed_python(
        ID,
        source(),
        source_identity(),
        input.file.clone(),
        control,
        result,
    );
    result.coverage.scope = "Static textual model hygiene only; no rendering, query parsing, datasource resolution or live health validation".into();
    result.coverage.limitations.push("Unknown plugin panels receive common textual rules only. PromQL heuristics require an explicit Prometheus datasource type.".into());
    let Some((executable, payload)) = invocation else {
        return;
    };
    let reply = parse_reply(&payload);
    let Some(reply) = reply else {
        result.execution.status = Status::Failed;
        result.coverage.state = "not_established".into();
        result.error("invalid_task_response", "The task did not return a valid complete response for its installed identity and version.");
        return;
    };
    if let Some(code) = &reply.error_code {
        result.execution.status = if code == "unsupported_runtime" {
            result.effect_outcome = "not_attempted".into();
            Status::Unsupported
        } else {
            Status::Failed
        };
        result.coverage.state = "not_established".into();
        result.error(code, model_error(code).expect("validated static error"));
        return;
    }
    let partial = reply.status == "partial";
    result.execution.status = if partial {
        Status::Partial
    } else {
        Status::Succeeded
    };
    result.coverage.state = if partial { "partial" } else { "complete" }.into();
    if partial {
        result.error("task_findings_truncated", "Finding fields or the finding list were shortened to enforce structured output bounds.");
    }
    result.data["runtime"] = json!({"executable":executable,"version":reply.runtime_version});
    result.data["input"] =
        json!({"file":input.file,"digest":reply.input_digest,"bytes":reply.input_bytes});
    result.data["checked_panels"] = json!(reply.checked_panels);
    result.data["findings"] = json!(reply.findings);
    result.data["findings_total"] = json!(reply.findings_total);
    result.data["findings_truncated"] = json!(reply.findings_truncated);
    result.data["fields_truncated"] = json!(reply.fields_truncated);
    result.data["coverage"] = json!({"state":result.coverage.state,"scope":result.coverage.scope,"limitations":result.coverage.limitations});
}

fn run_fixed_python(
    id: &str,
    source: &str,
    identity: Value,
    argument: String,
    control: &RunControl,
    result: &mut OperationResult,
) -> Option<(String, String)> {
    result.data = json!({"task":{"id":id,"version":1},"source":identity});
    let mut prepared = match python() {
        Ok(prepared) => prepared,
        Err(problem) => {
            result.reject(problem);
            return None;
        }
    };
    let executable = prepared.executable.to_string_lossy().into_owned();
    result.resolved_target = prepared.identity();
    result.resolved_target["effects"] = json!("observe");
    result.resolved_target["task"] =
        json!({"id":id,"version":1,"binding_digest":digest(source.as_bytes())});
    // Only this private installed binding can place compiled-in Python in argv. Public
    // process.exec validation, including its per-argument limit, remains unchanged.
    prepared.input.args = vec![
        "-I".into(),
        "-S".into(),
        "-B".into(),
        "-c".into(),
        source.into(),
        argument,
    ];
    result.data = json!({});
    #[cfg(target_os = "linux")]
    process::run(prepared, result, control);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (prepared, control);
        return None;
    }
    let mut supervisor = std::mem::replace(&mut result.data, json!({}));
    if let Some(object) = supervisor.as_object_mut() {
        object.remove("effects");
        object.remove("profile");
    }
    result.data = json!({"task":{"id":id,"version":1},"source":identity,"cancellation_signal":supervisor.get("cancellation_signal").cloned().unwrap_or(Value::Null),"supervisor":supervisor});
    if result.data["supervisor"].get("process_id").is_none() {
        result
            .data
            .as_object_mut()
            .expect("task data object")
            .remove("supervisor");
    }
    if result.effect_outcome != "not_attempted" {
        result.effect_outcome = "not_applicable".into();
    }
    let payload = std::mem::take(&mut result.output.stdout);
    let stderr = std::mem::take(&mut result.output.stderr);
    if !matches!(result.execution.status, Status::Succeeded) {
        result.coverage.state = "partial".into();
        return None;
    }
    if result.output.stdout_truncated || result.output.stderr_truncated {
        result.execution.status = Status::Partial;
        result.coverage.state = "partial".into();
        result.error(
            "task_output_incomplete",
            "Structured task output was truncated; no task results were accepted.",
        );
        return None;
    }
    if payload.len() > MAX_CHILD_RESPONSE || !stderr.is_empty() {
        invalid_response(result);
        return None;
    }
    Some((executable, payload))
}

fn invalid_response(result: &mut OperationResult) {
    result.execution.status = Status::Failed;
    result.coverage.state = "not_established".into();
    result.error(
        "invalid_task_response",
        "The task did not return a valid complete response for its installed identity and version.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_checker_matches_pinned_upstream_bytes() {
        assert_eq!(
            digest(CHECKER.as_bytes()),
            "sha256:8a109e921332b2da99885cda111ad6b6e7d22965291fd2bc4ddd0199876ec5b2"
        );
        assert!(
            source().len() < 128 * 1024,
            "one Linux argv element must fit MAX_ARG_STRLEN"
        );
    }
    #[test]
    fn malformed_or_mismatched_child_protocol_cannot_be_accepted() {
        let base = json!({"task":{"id":ID,"version":1},"status":"complete","runtime_version":"3.11.2","input_digest":digest(b"{}"),"input_bytes":2,"checked_panels":1,"findings":[],"findings_total":0,"findings_truncated":false,"fields_truncated":false,"error_code":null});
        assert!(valid_reply(&serde_json::from_value(base.clone()).unwrap()));
        assert!(parse_reply(&base.to_string()).is_some());
        for (field, value) in [
            ("task", json!([ID, 1])),
            ("findings", json!([["panel-title", "where", "detail"]])),
        ] {
            let mut invalid = base.clone();
            invalid[field] = value;
            assert!(
                parse_reply(&invalid.to_string()).is_none(),
                "{field} must contain objects, not struct sequences"
            );
        }
        for (field, value) in [
            ("status", json!("partial")),
            ("checked_panels", json!(0)),
            ("findings_total", json!(1)),
            ("task", json!({"id":ID,"version":2})),
            ("input_digest", json!("invalid")),
            ("runtime_version", json!("3.10.9")),
        ] {
            let mut invalid = base.clone();
            invalid[field] = value;
            assert!(
                !valid_reply(&serde_json::from_value(invalid).unwrap()),
                "{field}"
            );
        }
        for field in ["input_digest", "error_code"] {
            let mut invalid = base.clone();
            invalid.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<ChildReply>(invalid).is_err(),
                "{field} must be present even when null"
            );
        }
        let mut invalid = base;
        invalid["override"] = json!(true);
        assert!(serde_json::from_value::<ChildReply>(invalid).is_err());
    }
}
