//! Explicit bounded observation fan-out. A persisted child intent never authorizes replay.
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
fn keys(v: &Value, k: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|m| m.len() == k.len() && k.iter().all(|k| m.contains_key(*k)))
}
fn active(c: &RunControl, d: Instant) -> Result<(), Fault> {
    if c.signal.load(Ordering::Relaxed) != 0 {
        Err(Fault::new("fleet_cancelled"))
    } else if Instant::now() >= d {
        Err(Fault::new("fleet_deadline_exceeded"))
    } else {
        Ok(())
    }
}
fn initial() -> Value {
    json!({"format_version":1,"action":"status","run":null,"counts":{"succeeded":0,"failed":0,"unknown":0,"skipped":0,"pending":0},"targets":[],"aggregate":"unknown","aggregate_effect_outcome":"unknown","cancellation_signal":null})
}
pub(crate) fn run(
    request: &Request,
    c: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    run_expected(request, c, context, start, result, None);
}
pub(crate) fn run_expected(
    request: &Request,
    c: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
    expected: Option<&Value>,
) {
    result.data = initial();
    result.record_mode = match request.record.as_str() {
        "always" => "always",
        "auto" => "auto",
        _ => "never",
    };
    result.coverage.scope="Frozen target-set execution of the fixed observational error-budget calculator; supplied measurements and runner metadata do not establish current service health.".into();
    #[cfg(target_os = "linux")]
    let outcome = controller::run(request, c, context, start, result, expected);
    #[cfg(not(target_os = "linux"))]
    let outcome: Result<(), Fault> = Err(Fault::new("fleet_platform_unsupported"));
    if let Err(error) = outcome {
        if error.effects || result.effect_outcome == "succeeded" {
            result.effect_outcome = "unknown".into();
        }
        result.execution.status = if result.effect_outcome == "unknown" {
            Status::Unknown
        } else {
            match error.code {
                "fleet_cancelled" => Status::Cancelled,
                "fleet_deadline_exceeded" => Status::TimedOut,
                "fleet_store_required"
                | "fleet_inventory_required"
                | "fleet_platform_unsupported" => Status::Unsupported,
                "fleet_worker_busy" | "fleet_request_conflict" | "fleet_empty_selection" => {
                    Status::Denied
                }
                code if code.starts_with("invalid_") => Status::Denied,
                _ => Status::Failed,
            }
        };
        result.error(error.code,"Fleet ownership, persistence or collection could not be established. Preserve the request identity; resume may only observe or cancel original child intents.");
        result.coverage.state = "not_established".into();
    }
    let signal = c.signal.load(Ordering::Relaxed);
    if signal != 0 {
        result.data["cancellation_signal"] = json!(signal);
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn freeze(
    input: &Value,
    context: &OperatorContext,
    c: &RunControl,
    d: Instant,
) -> Result<(Value, bool), (&'static str, bool)> {
    controller::freeze(input, context, c, d).map_err(|e| (e.code, e.effects))
}
