//! Explicit owner-bound observational scheduling. Durable intents never authorize replay.
#[cfg(target_os = "linux")]
mod calendar;
#[cfg(target_os = "linux")]
mod config;
#[cfg(target_os = "linux")]
mod controller;
#[cfg(target_os = "linux")]
mod model;
#[cfg(target_os = "linux")]
mod store;
use crate::{
    OperatorContext, RunControl,
    request::Request,
    result::{OperationResult, Status},
};
use serde_json::{Value, json};
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
fn hash(v: &Value) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "sha256:{}",
        Sha256::digest(serde_json::to_vec(v).unwrap())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
fn keys(v: &Value, keys: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|m| m.len() == keys.len() && keys.iter().all(|k| m.contains_key(*k)))
}
fn active(c: &RunControl, d: Instant) -> Result<(), Fault> {
    if c.signal.load(Ordering::Relaxed) != 0 {
        Err(Fault::new("schedule_cancelled"))
    } else if Instant::now() >= d {
        Err(Fault::new("schedule_deadline_exceeded"))
    } else {
        Ok(())
    }
}
fn initial() -> Value {
    json!({"format_version":1,"action":"status","schedules":[],"occurrences":[],"preview":[],"omitted_occurrences":0,"cancellation_signal":null})
}
pub(crate) fn run(
    request: &Request,
    c: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
    worker: bool,
) {
    result.data = initial();
    result.record_mode = match request.record.as_str() {
        "always" => "always",
        "auto" => "auto",
        _ => "never",
    };
    result.coverage.scope="Explicit persistent observational schedule with pinned IANA rules and fleet authority. Supplied measurements do not establish current service health.".into();
    #[cfg(target_os = "linux")]
    let outcome = controller::run(request, c, context, start, result, worker);
    #[cfg(not(target_os = "linux"))]
    let outcome: Result<(), Fault> = Err(Fault::new("schedule_platform_unsupported"));
    if let Err(e) = outcome {
        if e.effects || result.effect_outcome == "succeeded" {
            result.effect_outcome = "unknown".into();
        }
        result.execution.status = if result.effect_outcome == "unknown" {
            Status::Unknown
        } else {
            match e.code {
                "schedule_cancelled" => Status::Cancelled,
                "schedule_deadline_exceeded" => Status::TimedOut,
                "schedule_store_required"
                | "schedule_platform_unsupported"
                | "schedule_worker_entry_required" => Status::Unsupported,
                code if code.starts_with("invalid_")
                    || matches!(
                        code,
                        "schedule_revision_conflict"
                            | "schedule_request_conflict"
                            | "schedule_worker_busy"
                            | "schedule_exists"
                            | "schedule_quota_exceeded"
                    ) =>
                {
                    Status::Denied
                }
                _ => Status::Failed,
            }
        };
        result.error(e.code,"Schedule state or pinned fleet authority could not be established. Retain original occurrence identities; uncertain intents are never replayed.");
        result.coverage.state = "not_established".into();
    }
    if matches!(result.execution.status, Status::Failed | Status::Unknown)
        && result.errors.is_empty()
    {
        result.error("schedule_work_incomplete","A scheduled occurrence failed or remains unresolved. Original occurrence and fleet request identities are retained; recovery never replays an intent.");
    }
    let signal = c.signal.load(Ordering::Relaxed);
    if signal != 0 {
        result.data["cancellation_signal"] = json!(signal);
    }
}
pub(crate) fn work(request: Request, c: &RunControl, context: &OperatorContext) -> OperationResult {
    let start = Instant::now();
    let mut result = OperationResult::new(&request);
    if let Err(e) = request.validate() {
        result.reject(e);
    } else if request.operation != "schedule.work" {
        result.error(
            "schedule_worker_entry_required",
            "The explicit scheduler entry only accepts schedule.work.",
        );
        result.execution.status = Status::Denied;
    } else {
        run(&request, c, context, start, &mut result, true);
    }
    result.finish(start);
    result
}
