use serde::Deserialize;
use serde_json::{Value, json};
use workbench_core::request::Request;

use crate::{grants::Grants, http::ApiError};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submission {
    submission_id: String,
    operation: String,
    operation_version: u64,
    #[serde(default)]
    root_id: Option<String>,
    inputs: Value,
    limits: Limits,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Limits {
    timeout_ms: u64,
    max_output_bytes: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    program: String,
    args: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Task {
    id: String,
    version: u64,
    input: Value,
}

pub(crate) struct Normalized {
    pub submission_id: String,
    pub request: Request,
    pub root_id: Option<String>,
    pub label: &'static str,
    pub identity: [u8; 32],
}

fn uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

pub(crate) fn normalize(value: Value, grants: &Grants) -> Result<Normalized, ApiError> {
    if !value.is_object() || !value["inputs"].is_object() || !value["limits"].is_object() {
        return Err(ApiError::invalid(
            "request",
            "The request, inputs and limits must be JSON objects.",
        ));
    }
    let had_root = value.get("root_id").is_some();
    let input: Submission = serde_json::from_value(value).map_err(|_| {
        ApiError::invalid(
            "request",
            "Missing, unknown or incorrectly typed request fields.",
        )
    })?;
    if !uuid(&input.submission_id) {
        return Err(ApiError::invalid(
            "submission_id",
            "Supply a UUID submission ID.",
        ));
    }
    if !grants.operations.contains(&input.operation) {
        return Err(ApiError::forbidden(
            "operation-not-granted",
            "The launcher did not grant this operation or its dependencies are unavailable.",
        ));
    }
    if input.operation_version != 1 {
        return Err(ApiError::invalid(
            "operation_version",
            "Only operation version 1 is available.",
        ));
    }
    let (inputs, label) = if input.operation == "task.run" {
        if had_root {
            return Err(ApiError::invalid(
                "root_id",
                "Numerical tasks do not accept a root selector.",
            ));
        }
        let task: Task = serde_json::from_value(input.inputs).map_err(|_| {
            ApiError::invalid(
                "inputs",
                "Supply only task id, version and numerical input.",
            )
        })?;
        if task.id != "error-budget" || task.version != 1 {
            return Err(ApiError::forbidden(
                "task-not-granted",
                "Only error-budget version 1 is granted.",
            ));
        }
        let numerical =
            workbench_core::tasks::normalize_error_budget_input(&task.input).map_err(|_| {
                ApiError::invalid(
                    "inputs.input",
                    "Input does not satisfy the numerical error-budget schema.",
                )
            })?;
        (
            json!({"id":task.id,"version":1,"input":numerical}),
            "Error budget",
        )
    } else {
        let root = input
            .root_id
            .as_ref()
            .and_then(|id| grants.roots.get(id))
            .ok_or_else(|| {
                ApiError::forbidden(
                    "root-not-granted",
                    "Select a root explicitly granted by the launcher.",
                )
            })?;
        let command: Command = serde_json::from_value(input.inputs).map_err(|_| {
            ApiError::invalid(
                "inputs",
                "Supply only the program and literal argument array.",
            )
        })?;
        if !matches!(command.program.as_str(), "git" | "rg")
            || command.args.len() > 64
            || command
                .args
                .iter()
                .any(|arg| arg.chars().count() > 4096 || arg.contains('\0'))
        {
            return Err(ApiError::invalid(
                "inputs",
                "Use git or rg with bounded literal arguments.",
            ));
        }
        let label = match (
            command.program.as_str(),
            command.args.first().map(String::as_str),
        ) {
            ("git", Some("status")) => "Working tree status",
            ("git", Some("diff")) => "Working tree diff",
            ("git", Some("log")) => "Recent commits",
            ("rg", Some("--files")) => "List files",
            ("rg", _) => "Search text",
            _ => "Inspect command",
        };
        (
            json!({"program":command.program,"args":command.args,"cwd":root.path}),
            label,
        )
    };
    let mut request = Request::new(&input.operation, inputs);
    request.operation_version = input.operation_version;
    request.limits.timeout_ms = input.limits.timeout_ms;
    request.limits.max_output_bytes = input.limits.max_output_bytes;
    request.validate().map_err(|_| {
        ApiError::invalid(
            "limits",
            "Timeout must be 1–300000ms and output 1024–2097152 bytes.",
        )
    })?;
    if serde_json::to_vec(&request)
        .expect("finite normalized request")
        .len()
        > workbench_core::request::MAX_REQUEST_BYTES
    {
        return Err(ApiError::invalid(
            "request",
            "The normalized core request exceeds64KiB.",
        ));
    }
    // Exclude the generated core request ID from the deterministic submission identity.
    use sha2::{Digest, Sha256};
    let identity = Sha256::digest(
        serde_json::to_vec(&json!({
            "operation":request.operation,"operation_version":request.operation_version,
            "inputs":request.inputs,"limits":request.limits,"root_id":input.root_id
        }))
        .expect("finite normalized request"),
    )
    .into();
    Ok(Normalized {
        submission_id: input.submission_id.to_ascii_lowercase(),
        request,
        root_id: input.root_id,
        label,
        identity,
    })
}
