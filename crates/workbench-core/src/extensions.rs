//! Strict package storage and separately policy-pinned external execution.
#[cfg(target_os = "linux")]
mod discovery;
#[cfg(target_os = "linux")]
mod execution;
mod package;
#[cfg(target_os = "linux")]
mod policy;
#[cfg(target_os = "linux")]
mod protocol;
#[cfg(target_os = "linux")]
mod storage;
use crate::{
    OperatorContext, RunControl,
    request::{Problem, Request},
    result::{OperationResult, Status},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

#[derive(Debug)]
struct Fault {
    code: &'static str,
    effects: bool,
}
impl Fault {
    fn new(code: &'static str) -> Self {
        Self {
            code,
            effects: false,
        }
    }
    fn after_effects(mut self) -> Self {
        self.effects = true;
        self
    }
}
fn keys(v: &Value, required: &[&str], optional: &[&str]) -> bool {
    v.as_object().is_some_and(|m| {
        required.iter().all(|k| m.contains_key(*k))
            && m.keys()
                .all(|k| required.contains(&k.as_str()) || optional.contains(&k.as_str()))
            && m.values().all(|v| !v.is_null())
    })
}
fn text(v: &Value, max: usize) -> bool {
    v.as_str()
        .is_some_and(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
}
fn digest_string(s: &str) -> bool {
    s.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn digest(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::from("sha256:");
    for byte in Sha256::digest(bytes) {
        write!(&mut result, "{byte:02x}").expect("string writing");
    }
    result
}
fn active(control: &RunControl, deadline: Instant) -> Result<(), Fault> {
    if control.signal.load(Ordering::Relaxed) != 0 {
        return Err(Fault::new("extension_cancelled"));
    }
    if Instant::now() >= deadline {
        return Err(Fault::new("extension_deadline_exceeded"));
    }
    #[cfg(target_os = "linux")]
    if let Some(fd) = control.result_fd {
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: a live initialized pollfd is borrowed by a nonblocking syscall.
        if unsafe { libc::poll(&mut poll, 1, 0) } > 0
            && poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
        {
            return Err(Fault::new("extension_consumer_closed"));
        }
    }
    Ok(())
}
fn validate(request: &Request) -> Result<(), Problem> {
    let input = &request.inputs;
    let valid = match request.operation.as_str() {
        "extension.list" => keys(input, &[], &[]),
        "extension.inspect" | "extension.install" => {
            let valid_keys = if request.operation == "extension.install" {
                keys(input, &["package", "expected_package_sha256"], &[])
            } else {
                keys(input, &["package"], &["expected_package_sha256"])
            };
            valid_keys
                && text(&input["package"], 1024)
                && input["package"].as_str().is_some_and(|p| {
                    std::path::Path::new(p).is_absolute()
                        && !std::path::Path::new(p)
                            .components()
                            .any(|c| matches!(c, std::path::Component::ParentDir))
                })
                && input
                    .get("expected_package_sha256")
                    .is_none_or(|v| v.as_str().is_some_and(digest_string))
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(Problem::invalid(
            "invalid_extension_input",
            "Use an absolute bounded package path and supported digest fields only; install requires the package SHA-256 pin.",
        ))
    }
}
fn failure(fault: Fault, control: &RunControl, result: &mut OperationResult) {
    result.execution.status = match fault.code {
        "extension_cancelled" | "extension_consumer_closed" => Status::Cancelled,
        "extension_deadline_exceeded" => Status::TimedOut,
        "extension_package_digest_mismatch" | "extension_package_conflict" => Status::Denied,
        "invalid_extension_package" | "extension_package_too_large" | "extension_source_unsafe" => {
            Status::Denied
        }
        "extension_store_required" | "extension_unsupported_platform" => Status::Unsupported,
        "job_binding_changed"
        | "extension_policy_required"
        | "extension_policy_invalid"
        | "extension_not_authorized"
        | "extension_ambiguous_operation"
        | "invalid_extension_arguments"
        | "extension_request_too_large" => Status::Denied,
        _ => Status::Failed,
    };
    if fault.effects {
        result.effect_outcome = "unknown".into();
    }
    let signal = control.signal.load(Ordering::Relaxed);
    if signal != 0 {
        result.data["cancellation_signal"] = json!(signal);
    }
    result.coverage.state = "not_established".into();
    result.error(fault.code, if result.data["supervisor"]["process_id"].is_number() {
        "The extension invocation stopped after child execution began; original child and cleanup outcomes are retained."
    } else if fault.effects {
        "Package management stopped after local store effects; publication or durability may be incomplete. Preserve remaining files for explicit recovery. No package code executed."
    } else { "The extension package or private store could not be admitted. No package code executed; no new package was installed." });
}
pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    if let Err(problem) = validate(request) {
        result.reject(problem);
        return;
    }
    let action = request.operation.strip_prefix("extension.").unwrap();
    result.data = json!({"packages":[],"installed_count":0,"total_packages":0,"omitted_packages":0,"action":action,
        "enabled":false,"execution_available":false,"provenance_verified":false,"cancellation_signal":null,"scope":if action=="inspect"{"package_inspection"}else{"installed_inventory"},"policy_sha256":null});
    result.coverage.scope = "Strict inert package admission and verified private inventory; executable behavior, provenance authenticity and extension protocol conformance are not established.".into();
    result.coverage.limitations.push("Installation never executes code. Effective execution availability requires a separate exact startup policy; capability effects and provenance remain unverified declarations.".into());
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    #[cfg(target_os = "linux")]
    let operation = storage::run(request, control, context, deadline, result);
    #[cfg(not(target_os = "linux"))]
    let operation = {
        let _ = (context, deadline);
        Err(Fault::new("extension_unsupported_platform"))
    };
    if let Err(fault) = operation {
        failure(fault, control, result);
        return;
    }
    // Bound the entire data object, not just each manifest. Keep durable outcome/counts
    // even when no individual description fits the requested detail budget.
    let total = result.data["packages"].as_array().unwrap().len();
    result.data["total_packages"] = json!(total);
    while serde_json::to_vec(&result.data).expect("data").len()
        > result
            .effective_limits
            .max_output_bytes
            .min(crate::request::MAX_RESULT_BYTES - 64 * 1024)
    {
        if result.data["packages"]
            .as_array_mut()
            .unwrap()
            .pop()
            .is_none()
        {
            break;
        }
        result.data["omitted_packages"] =
            json!(total - result.data["packages"].as_array().unwrap().len());
    }
    if result.data["omitted_packages"] != 0 {
        result.execution.status = Status::Partial;
        result.coverage.state = "partial".into();
        result.coverage.limitations.push("Package descriptions omitted to honor max_output_bytes; inventory and installation outcome remain explicit.".into());
    } else {
        result.coverage.state = "complete".into();
    }
    if let Err(mut fault) = active(control, deadline) {
        fault.effects = result.effect_outcome == "succeeded";
        failure(fault, control, result);
    }
}

pub(crate) fn execute(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    #[cfg(target_os = "linux")]
    execution::run(request, control, context, start, result);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (request, context, start);
        failure(
            Fault::new("extension_unsupported_platform"),
            control,
            result,
        );
    }
}
pub(crate) fn discover(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    #[cfg(target_os = "linux")]
    discovery::run(request, control, context, start, result);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (request, context, start);
        failure(
            Fault::new("extension_unsupported_platform"),
            control,
            result,
        );
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn job_binding(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    deadline: Instant,
) -> Result<Value, Problem> {
    execution::job_binding(request, control, context, deadline).map_err(|fault| {
        Problem::invalid(
            fault.code,
            "The current extension package and explicit policy could not be admitted for this job.",
        )
    })
}
#[cfg(target_os = "linux")]
pub(crate) fn execute_job(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
    expected: &Value,
) {
    execution::run_bound(request, control, context, start, result, Some(expected));
}
