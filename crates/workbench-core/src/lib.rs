//! Shared, bounded operation semantics for the experimental `save` CLI.
mod change;
mod context;
mod investigation;
pub use investigation::{
    InvestigationGrant, execute_with_investigation_grant, investigation_grant,
    normalize_investigation_input,
};
mod evidence;
mod extensions;
mod fleet;
mod jobs;
mod runner;
mod schedule;
pub use evidence::EvidenceStoreOptions;
pub use runner::{RunnerError, serve_runner};
mod diagnostics;
pub use diagnostics::{
    DiagnosticsGrant, diagnostics_grant, diagnostics_pack_metadata, normalize_diagnostics_input,
};
pub mod discovery;
mod grafana;
pub use grafana::{
    GrafanaConfigDescription, GrafanaConfigIdentity, GrafanaTargetDescription,
    grafana_config_description,
};
mod process;
mod queries;
mod read_launcher;
mod read_profile;
mod runbooks;
pub use read_launcher::{ReadProfileLauncher, read_launcher_entry};
pub use read_profile::{ReadPolicyDescription, ReadPolicyRoot, read_policy_description};
pub mod request;
pub mod result;
pub mod tasks;
mod toil;

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
    /// Explicit owner-bound persistent observational schedule store.
    pub schedule_state_dir: Option<std::path::PathBuf>,
    /// Explicit frozen-inventory controller inputs; never a wire grant.
    pub fleet_inventory_path: Option<std::path::PathBuf>,
    /// Explicit private durable fleet parent store.
    pub fleet_state_dir: Option<std::path::PathBuf>,
    /// Explicit authenticated remote controller profile; never a wire grant.
    pub runner_config_path: Option<std::path::PathBuf>,
    /// Explicit private durable local-job store, never a wire field or hosted grant.
    pub job_state_dir: Option<std::path::PathBuf>,
    /// Startup-only local worker ceiling; at most one hour, independent of foreground limits.
    pub job_max_runtime_ms: Option<u64>,
    /// Independent startup-only exact extension execution grants under ordinary operator authority.
    pub extension_policy_path: Option<std::path::PathBuf>,
    /// Explicit private inert extension inventory; never a wire field or hosted grant.
    pub extension_state_dir: Option<std::path::PathBuf>,
    /// Explicit startup location for the exact reviewed promtool binary; never a wire grant.
    pub promtool_path: Option<std::path::PathBuf>,
    /// Explicit separate opt-in toil metadata store; never inherited by hosted callers.
    pub toil_state_dir: Option<std::path::PathBuf>,
    /// Explicit private guided-runbook journal root, never a wire field or hosted grant.
    pub runbook_state_dir: Option<std::path::PathBuf>,
    /// Explicit local evidence storage policy; never supplied by wire input or a hosted grant.
    pub evidence_store: Option<EvidenceStoreOptions>,
    /// Explicit startup-only diagnostics destination/configuration binding; never a wire grant.
    pub diagnostics_config_path: Option<std::path::PathBuf>,
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
        Identities {
            policy: expected_policy_digest,
            ..Identities::default()
        },
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
        Identities {
            policy: expected_policy_digest,
            grafana: expected_grafana_identity,
            required: true,
            ..Identities::default()
        },
    )
}

/// Explicit hosted diagnostic grant path. The caller admits the selected aliases with
/// `diagnostics_grant`; generic hosted execution continues to refuse diagnostics.
pub fn execute_with_diagnostic_grant(
    request: Request,
    control: &RunControl,
    context: &OperatorContext,
    expected_policy_digest: Option<&str>,
    grant: &DiagnosticsGrant,
) -> OperationResult {
    execute_with_identities(
        request,
        control,
        context,
        Identities {
            policy: expected_policy_digest,
            diagnostics: Some(grant),
            required: true,
            ..Identities::default()
        },
    )
}

#[derive(Default)]
struct Identities<'a> {
    policy: Option<&'a str>,
    grafana: Option<&'a GrafanaConfigIdentity>,
    diagnostics: Option<&'a DiagnosticsGrant>,
    required: bool,
}

fn execute_with_identities(
    request: Request,
    control: &RunControl,
    context: &OperatorContext,
    identities: Identities<'_>,
) -> OperationResult {
    let start = Instant::now();
    let mut result = OperationResult::new(&request);
    if let Err(problem) = request.validate() {
        result.reject(problem);
    } else if identities.required && request.operation.starts_with("change.") {
        result.reject(Problem::unsupported("change_grants_unavailable", "Controlled changes require an authenticated operator profile and independently granted runner authority."));
    } else if identities.required && request.operation.starts_with("schedule.") {
        result.reject(Problem::unsupported(
            "schedule_grants_unavailable",
            "Persistent schedules require explicit local operator authority.",
        ));
    } else if identities.required && request.operation.starts_with("fleet.") {
        result.reject(Problem::unsupported("fleet_grants_unavailable","Fleet inventory and persistence are explicit operator authority, unavailable to hosted callers."));
    } else if identities.required && request.operation.starts_with("runner.") {
        result.reject(Problem::unsupported(
            "runner_grants_unavailable",
            "Remote runner profiles are operator startup authority, unavailable to hosted callers.",
        ));
    } else if identities.required && request.operation.starts_with("job.") {
        result.reject(Problem::unsupported(
            "job_grants_unavailable",
            "Durable jobs are available only through the explicit local operator store and worker.",
        ));
    } else if identities.required
        && (request.operation.starts_with("extension.") || request.operation.starts_with("ext."))
    {
        result.reject(Problem::unsupported("extension_grants_unavailable", "Hosted extension grants are unavailable; use explicit operator-local package management."));
    } else if identities.required && request.operation == "config.check" {
        result.reject(Problem::unsupported("config_grants_unavailable", "Hosted configuration-check grants are unavailable; use the explicit operator-local installed validator."));
    } else if identities.required && request.operation.starts_with("toil.") {
        result.reject(Problem::unsupported(
            "toil_grants_unavailable",
            "Hosted toil grants are unavailable; use an explicit operator-local metadata store.",
        ));
    } else if identities.required && request.operation == "context.resolve" {
        result.reject(Problem::unsupported(
            "context_grants_unavailable",
            "Hosted context resolution requires an explicit pinned investigation grant.",
        ));
    } else if identities.required && request.operation == "report.create" {
        result.reject(Problem::unsupported("report_grants_unavailable", "Report grants are unavailable for hosted adapters; use explicit operator evidence-store bindings."));
    } else if identities.required && request.operation.starts_with("runbook.") {
        result.reject(Problem::unsupported(
            "runbook_grants_unavailable",
            "Guided runbook grants are not implemented for hosted adapters; use explicit operator CLI bindings.",
        ));
    } else if identities.required && request.operation.starts_with("evidence.") {
        result.reject(Problem::unsupported(
            "evidence_grants_unavailable",
            "Evidence store and collection grants are not implemented for hosted adapters; use the explicit operator CLI binding.",
        ));
    } else if identities.required && request.operation == "query.run" {
        result.reject(Problem::unsupported(
            "query_grants_unavailable",
            "Saved query grants are not implemented for hosted adapters; use the explicit operator CLI binding.",
        ));
    } else if identities.required
        && identities.diagnostics.is_none()
        && request.operation == "diagnostics.run"
    {
        result.reject(Problem::unsupported("diagnostics_grants_unavailable", "Diagnostics grants are not implemented for hosted adapters; use the explicit operator CLI binding."));
    } else if identities.required
        && identities.policy.is_none()
        && matches!(
            request.operation.as_str(),
            "process.exec" | "command.inspect"
        )
    {
        result.reject(Problem::invalid("read_policy_identity_required", "A pinned command grant requires its trusted policy identity; unpinned execution is refused."));
    } else if identities.required
        && identities.grafana.is_none()
        && matches!(
            request.operation.as_str(),
            "grafana.dashboard.get" | "grafana.query"
        )
    {
        result.reject(Problem::invalid("grafana_identity_required", "A pinned Grafana grant requires its trusted configuration identity; unpinned execution is refused."));
    } else if identities.policy.is_some()
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
        dispatch(&request, control, context, &identities, start, &mut result);
    }
    result.finish(start);
    result
}

/// Fixed collection children only. Keep a real child clock while narrowing its work to the
/// absolute parent deadline; no caller-selected operation, nested collection or hosted fallback.
pub(crate) fn execute_collection_child(
    mut request: Request,
    control: &RunControl,
    context: &OperatorContext,
    expected_grafana_identity: Option<&GrafanaConfigIdentity>,
    deadline: Instant,
) -> OperationResult {
    let start = Instant::now();
    let mut result = OperationResult::new(&request);
    let remaining = deadline.saturating_duration_since(start).as_millis();
    if remaining == 0 {
        result.execution.status = Status::TimedOut;
        result.error(
            "parent_deadline_exceeded",
            "The collection deadline expired before this child could start.",
        );
    } else if control.signal.load(Ordering::Relaxed) != 0 {
        result.execution.status = Status::Cancelled;
        result.data = json!({"cancellation_signal":control.signal.load(Ordering::Relaxed)});
        result.error(
            "cancelled",
            "The collection was cancelled before this child could start.",
        );
    } else {
        request.limits.timeout_ms = request.limits.timeout_ms.min(remaining.min(60_000) as u64);
        result.effective_limits.timeout_ms = request.limits.timeout_ms;
        if let Err(problem) = request.validate() {
            result.reject(problem);
        } else {
            match request.operation.as_str() {
                "context.resolve" => context::run(&request, control, context, start, &mut result),
                "query.run" if expected_grafana_identity.is_some() => grafana::run_with_deadline(
                    &request, control, context, expected_grafana_identity, start, deadline, &mut result,
                ),
                _ => result.reject(Problem::unsupported(
                    "unsupported_collection_child",
                    "Collection permits only its fixed context resolution and pinned saved queries.",
                )),
            }
        }
    }
    result.finish(start);
    result
}

/// Only installed observation capabilities may be dispatched by guided runbooks.
/// Keep a single parent deadline; never recurse through public execution.
pub(crate) fn execute_runbook_child(
    mut request: Request,
    control: &RunControl,
    context: &OperatorContext,
    deadline: Instant,
) -> OperationResult {
    let start = Instant::now();
    let mut result = OperationResult::new(&request);
    let remaining = deadline.saturating_duration_since(start).as_millis();
    if remaining == 0 {
        result.execution.status = Status::TimedOut;
        result.error(
            "parent_deadline_exceeded",
            "The runbook deadline expired before this check.",
        );
    } else if control.signal.load(Ordering::Relaxed) != 0 {
        result.execution.status = Status::Cancelled;
        result.data = json!({"cancellation_signal":control.signal.load(Ordering::Relaxed)});
        result.error("cancelled", "The runbook was cancelled before this check.");
    } else {
        request.limits.timeout_ms = request.limits.timeout_ms.min(remaining.min(300_000) as u64);
        result.effective_limits.timeout_ms = request.limits.timeout_ms;
        if let Err(problem) = request.validate() {
            result.reject(problem);
        } else {
            match request.operation.as_str() {
                "evidence.show" => evidence::manage(&request, control, context, start, &mut result),
                "evidence.compare" => {
                    evidence::compare::run(&request, control, context, start, &mut result)
                }
                _ => result.reject(Problem::unsupported(
                    "unsupported_runbook_step",
                    "Only installed evidence observations are permitted as runbook checks.",
                )),
            }
        }
    }
    result.finish(start);
    result
}

fn dispatch(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    identities: &Identities<'_>,
    start: Instant,
    result: &mut OperationResult,
) {
    match request.operation.as_str() {
        "runbook.list" if request.inputs == json!({}) => {
            result.data = runbooks::list();
            result.effect_outcome = "not_applicable".into();
        }
        "runbook.list" => result.reject(Problem::invalid(
            "invalid_runbook_input",
            "Runbook listing accepts an empty input object only.",
        )),
        "runbook.show" => runbooks::show(&request.inputs, result),
        "runbook.run" | "runbook.resume" | "runbook.status" => {
            runbooks::run(request, control, context, start, result)
        }
        "toil.configure" | "toil.record" | "toil.capture" | "toil.annotate" | "toil.summary"
        | "toil.prune" | "toil.delete" => toil::run(request, control, context, start, result),
        operation if operation.starts_with("change.") => {
            change::client::run(request, control, context, start, result)
        }
        operation if operation.starts_with("schedule.") => {
            schedule::run(request, control, context, start, result, false)
        }
        operation if operation.starts_with("fleet.") => {
            fleet::run(request, control, context, start, result)
        }
        operation if operation.starts_with("runner.") => {
            runner::run(request, control, context, start, result)
        }
        operation if operation.starts_with("job.") => {
            jobs::run(request, control, context, start, result)
        }
        operation if operation.starts_with("ext.") => {
            extensions::execute(request, control, context, start, result)
        }
        "extension.inspect" | "extension.install" | "extension.list" => {
            extensions::run(request, control, context, start, result)
        }
        "config.check" => tasks::config_check::run(request, control, context, start, result),
        "report.create" => evidence::report::run(request, control, context, start, result),
        "evidence.open" | "evidence.replay" => {
            evidence::offline::run(request, control, context, start, result)
        }
        "evidence.collect" => evidence::collect::run(request, control, context, start, result),
        "evidence.compare" => evidence::compare::run(request, control, context, start, result),
        "evidence.packs.describe" => evidence::pack::describe(&request.inputs, result),
        "evidence.list" | "evidence.show" | "evidence.protect" | "evidence.unprotect"
        | "evidence.delete" | "evidence.prune" | "evidence.recover" => {
            evidence::manage(request, control, context, start, result)
        }
        "diagnostics.run" => diagnostics::run(
            request,
            control,
            context,
            identities.diagnostics,
            start,
            result,
        ),
        "diagnostics.describe" => diagnostics::describe(&request.inputs, result),
        "context.resolve" => context::run(request, control, context, start, result),
        "grafana.dashboard.get" | "grafana.query" | "query.run" => {
            grafana::run(request, control, context, identities.grafana, start, result)
        }
        "queries.describe" => queries::describe(&request.inputs, result),
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
                identities.policy,
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
        "capability.list"
        | "doctor"
        | "task.list"
        | "diagnostics.list"
        | "queries.list"
        | "evidence.packs.list" => {
            if request.inputs.as_object().is_some_and(|map| map.is_empty()) {
                result.effect_outcome = "not_applicable".into();
                result.data = if request.operation == "evidence.packs.list" {
                    evidence::pack::list()
                } else if request.operation == "queries.list" {
                    queries::list()
                } else if request.operation == "diagnostics.list" {
                    diagnostics::list()
                } else if request.operation == "task.list" {
                    tasks::list()
                } else if request.operation == "doctor" {
                    discovery::doctor()
                } else {
                    discovery::list()
                };
                if request.operation == "capability.list"
                    && !identities.required
                    && context.extension_state_dir.is_some()
                {
                    extensions::discover(request, control, context, start, result);
                }
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
                    None if input.operation.starts_with("ext.")
                        && !identities.required
                        && context.extension_state_dir.is_some() =>
                    {
                        extensions::discover(request, control, context, start, result)
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

/// Explicit operator-local durable worker entry; generic/hosted dispatch cannot select it.
/// The ordinary envelope deadline bounds admission. The immutable admitted job supplies
/// a separate host-capped runtime, reported in the worker and child effective limits.
pub fn work_job(
    request: Request,
    control: &RunControl,
    context: &OperatorContext,
) -> OperationResult {
    jobs::work(request, control, context)
}

/// Explicit foreground scheduler pass. Generic and hosted requests cannot start it.
pub fn work_schedule(
    request: Request,
    control: &RunControl,
    context: &OperatorContext,
) -> OperationResult {
    schedule::work(request, control, context)
}
