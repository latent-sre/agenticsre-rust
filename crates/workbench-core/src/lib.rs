//! Shared, bounded operation semantics for the experimental `save` CLI.
mod context;
pub mod discovery;
mod grafana;
pub use grafana::{
    GrafanaConfigDescription, GrafanaConfigIdentity, GrafanaTargetDescription,
    grafana_config_description,
};
mod process;
mod read_launcher;
mod read_profile;
pub use read_launcher::{ReadProfileLauncher, read_launcher_entry};
pub use read_profile::{ReadPolicyDescription, ReadPolicyRoot, read_policy_description};
pub mod request;
pub mod result;
pub mod tasks;

use request::{Problem, Request};
use result::{OperationResult, Status};
use serde::Deserialize;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Instant;

/// Explicit operator startup configuration, never a wire-request field or a grant.
#[derive(Default)]
pub struct OperatorContext {
    /// Explicit offline fixture export and bounded freshness policy; never wire fields.
    pub context_source_path: Option<std::path::PathBuf>,
    pub context_max_age_days: Option<u16>,
    pub config_path: Option<std::path::PathBuf>,
    /// Narrow command execution using this explicit trusted policy; never read from a request.
    pub read_policy_path: Option<std::path::PathBuf>,
    /// Fixed launcher entry implemented by the hosting binary, pinned to its current image.
    pub read_profile_launcher: Option<ReadProfileLauncher>,
}

#[derive(Default)]
pub struct RunControl {
    pub signal: Arc<AtomicUsize>,
    /// Linux descriptor checked for consumer hangup; does not stream child output.
    pub result_fd: Option<i32>,
}

pub fn execute(request: Request, control: &RunControl) -> OperationResult {
    execute_with_context(request, control, &OperatorContext::default())
}

pub fn execute_with_context(
    request: Request,
    control: &RunControl,
    context: &OperatorContext,
) -> OperationResult {
    execute_with_policy_identity(request, control, context, None)
}

/// Execute with a launcher-pinned policy digest, checked on the same policy bytes used to admit
/// the command. The trusted adapter supplies this identity; it is never a wire-request field.
pub fn execute_with_policy_identity(
    request: Request,
    control: &RunControl,
    context: &OperatorContext,
    expected_policy_digest: Option<&str>,
) -> OperationResult {
    execute_with_identities(
        request,
        control,
        context,
        expected_policy_digest,
        None,
        false,
    )
}

/// Execute under launcher-pinned authority. Commands require the policy identity and Grafana
/// observations require the configuration identity; neither can fall back to operator authority.
/// These identities are trusted startup inputs, never fields accepted from a wire request.
pub fn execute_with_authority_identities(
    request: Request,
    control: &RunControl,
    context: &OperatorContext,
    expected_policy_digest: Option<&str>,
    expected_grafana_identity: Option<&GrafanaConfigIdentity>,
) -> OperationResult {
    execute_with_identities(
        request,
        control,
        context,
        expected_policy_digest,
        expected_grafana_identity,
        true,
    )
}

fn execute_with_identities(
    request: Request,
    control: &RunControl,
    context: &OperatorContext,
    expected_policy_digest: Option<&str>,
    expected_grafana_identity: Option<&GrafanaConfigIdentity>,
    require_identities: bool,
) -> OperationResult {
    let start = Instant::now();
    let mut result = OperationResult::new(&request);
    if let Err(problem) = request.validate() {
        result.reject(problem);
    } else if require_identities
        && expected_policy_digest.is_none()
        && matches!(
            request.operation.as_str(),
            "process.exec" | "command.inspect"
        )
    {
        result.reject(Problem::invalid("read_policy_identity_required", "A pinned command grant requires its trusted policy identity; unpinned execution is refused."));
    } else if require_identities
        && expected_grafana_identity.is_none()
        && matches!(
            request.operation.as_str(),
            "grafana.dashboard.get" | "grafana.query"
        )
    {
        result.reject(Problem::invalid("grafana_identity_required", "A pinned Grafana grant requires its trusted configuration identity; unpinned execution is refused."));
    } else if expected_policy_digest.is_some()
        && context.read_policy_path.is_none()
        && matches!(
            request.operation.as_str(),
            "process.exec" | "command.inspect"
        )
    {
        result.reject(Problem::invalid("read_policy_required", "A pinned command grant requires its explicit trusted read policy; generic execution is refused."));
    } else if control.signal.load(Ordering::Relaxed) != 0 {
        result.execution.status = Status::Cancelled;
        result.error("cancelled", "The invocation was cancelled before dispatch.");
        result.data = json!({"cancellation_signal":control.signal.load(Ordering::Relaxed)});
    } else {
        dispatch(
            &request,
            control,
            context,
            expected_policy_digest,
            expected_grafana_identity,
            start,
            &mut result,
        );
    }
    result.finish(start);
    result
}

fn dispatch(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    expected_policy_digest: Option<&str>,
    expected_grafana_identity: Option<&GrafanaConfigIdentity>,
    start: Instant,
    result: &mut OperationResult,
) {
    match request.operation.as_str() {
        "context.resolve" => context::run(request, control, context, start, result),
        "grafana.dashboard.get" | "grafana.query" => grafana::run(
            request,
            control,
            context,
            expected_grafana_identity,
            start,
            result,
        ),
        "task.run" => tasks::run(request, control, result),
        "task.describe" => tasks::describe(&request.inputs, result),
        "process.exec" | "command.inspect" if context.read_policy_path.is_some() => {
            read_profile::run_with_policy_identity(
                request,
                control,
                context
                    .read_policy_path
                    .as_deref()
                    .expect("guarded read policy"),
                context.read_profile_launcher.as_ref(),
                expected_policy_digest,
                start,
                result,
            );
        }
        "process.exec" | "command.inspect" => match process::prepare(&request.inputs) {
            Ok(prepared) => {
                result.resolved_target = prepared.identity();
                if request.operation == "command.inspect" {
                    result.effect_outcome = "not_applicable".into();
                    result.data = json!({"would_execute":true,"execution_performed":false,"effects":"unclassified","profile":"operator-local", "argument_count":prepared.input.args.len(),
                        "environment":process::CHILD_ENV.iter().map(|(k,v)| ((*k).to_owned(), json!(v))).collect::<serde_json::Map<_,_>>(), "stdin":"closed"});
                } else {
                    #[cfg(target_os = "linux")]
                    process::run(prepared, result, control);
                    #[cfg(not(target_os = "linux"))]
                    result.reject(Problem::unsupported(
                        "unsupported_platform",
                        "Process execution is currently implemented only on Linux.",
                    ));
                }
            }
            Err(problem) => result.reject(problem),
        },
        "capability.list" | "doctor" | "task.list" => {
            if request.inputs.as_object().is_some_and(|map| map.is_empty()) {
                result.effect_outcome = "not_applicable".into();
                result.data = if request.operation == "task.list" {
                    tasks::list()
                } else if request.operation == "doctor" {
                    discovery::doctor()
                } else {
                    discovery::list()
                };
            } else {
                result.reject(Problem::invalid(
                    "invalid_input",
                    "This offline operation accepts an empty inputs object only.",
                ));
            }
        }
        "capability.describe" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                operation: String,
            }
            match serde_json::from_value::<Input>(request.inputs.clone()) {
                Ok(input) => match discovery::capability(&input.operation) {
                    Some(capability) => {
                        result.effect_outcome = "not_applicable".into();
                        result.data = json!({"capability":capability,"availability":discovery::availability(&input.operation)});
                    }
                    None => result.reject(Problem::unsupported(
                        "unknown_operation",
                        "No installed capability has the requested operation ID.",
                    )),
                },
                Err(_) => result.reject(Problem::invalid(
                    "invalid_input",
                    "Capability description requires only an operation string.",
                )),
            }
        }
        _ => result.reject(Problem::unsupported(
            "unknown_operation",
            "No installed capability has the requested operation ID.",
        )),
    }
}
