use super::*;
use serde::{Deserialize, Serialize};

pub(super) const MAX_RUNTIME: u64 = 3_600_000;
pub(super) const MAX_RESULT: usize = 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Admission {
    pub format_version: u64,
    pub job_id: String,
    pub run_id: String,
    pub owner_uid: u32,
    pub request: Request,
    pub request_sha256: String,
    pub max_runtime_ms: u64,
    pub retain_events: usize,
    pub created_at: String,
    pub implementation: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Event {
    pub sequence: u64,
    pub at: String,
    pub kind: String,
    pub state: String,
    pub elapsed_ms: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    pub format_version: u64,
    pub job_id: String,
    pub run_id: String,
    pub request_sha256: String,
    pub state: String,
    pub dispatched_at: Option<String>,
    pub finished_at: Option<String>,
    pub attempt_id: Option<String>,
    pub cancel_requested: bool,
    pub last_sequence: u64,
    pub events: Vec<Event>,
    pub execution_status: Option<String>,
    pub effect_outcome: String,
    pub result_sha256: Option<String>,
}
pub(super) fn valid_id(id: &str) -> bool {
    id.strip_prefix("job-")
        .is_some_and(|tail| crate::evidence::store::valid_run_id(&format!("run-{tail}")))
}
pub(super) fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "sha256:{}",
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}
pub(super) fn fingerprint(
    request: &Request,
    runtime: u64,
    retention: usize,
    implementation: &Value,
    owner: u32,
) -> String {
    digest(&serde_json::to_vec(&json!({"request":request,"max_runtime_ms":runtime,"retain_events":retention,"implementation":implementation,"owner_uid":owner})).expect("fingerprint"))
}
pub(super) fn phase(state: &str) -> bool {
    matches!(
        state,
        "queued" | "running" | "cancelling" | "finalizing" | "terminal" | "recovery_required"
    )
}
pub(super) fn status(value: &str) -> bool {
    matches!(
        value,
        "succeeded"
            | "failed"
            | "partial"
            | "denied"
            | "unsupported"
            | "cancelled"
            | "timed_out"
            | "unknown"
    )
}
fn utc(value: &str) -> bool {
    value.len() <= 40
        && value.ends_with('Z')
        && time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .is_ok_and(|t| (1970..=9999).contains(&t.year()) && t.offset() == time::UtcOffset::UTC)
}
fn hash(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    })
}
fn attempt(value: &str) -> bool {
    value.len() <= 128
        && value
            .strip_prefix("attempt-")
            .is_some_and(|s| crate::evidence::store::valid_run_id(&format!("run-{s}")))
}
impl Admission {
    pub fn valid(&self, id: &str) -> bool {
        self.format_version == 1
            && self.job_id == id
            && valid_id(id)
            && crate::evidence::store::valid_run_id(&self.run_id)
            && utc(&self.created_at)
            && self.owner_uid == uid()
            && self.request.validate().is_ok()
            && supported(&self.request.operation)
            && (1..=MAX_RUNTIME).contains(&self.max_runtime_ms)
            && (1..=256).contains(&self.retain_events)
            && self.request_sha256
                == fingerprint(
                    &self.request,
                    self.max_runtime_ms,
                    self.retain_events,
                    &self.implementation,
                    self.owner_uid,
                )
    }
}
impl State {
    pub fn new(admission: &Admission) -> Self {
        let mut state = Self {
            format_version: 1,
            job_id: admission.job_id.clone(),
            run_id: admission.run_id.clone(),
            request_sha256: admission.request_sha256.clone(),
            state: "queued".into(),
            dispatched_at: None,
            finished_at: None,
            attempt_id: None,
            cancel_requested: false,
            last_sequence: 0,
            events: vec![],
            execution_status: None,
            effect_outcome: "not_attempted".into(),
            result_sha256: None,
        };
        state.event("queued", None, admission.retain_events);
        state
    }
    pub fn event(&mut self, kind: &str, elapsed_ms: Option<u64>, retention: usize) {
        self.last_sequence += 1;
        self.events.push(Event {
            sequence: self.last_sequence,
            at: crate::result::timestamp(),
            kind: kind.into(),
            state: self.state.clone(),
            elapsed_ms,
        });
        if self.events.len() > retention {
            self.events.drain(..self.events.len() - retention);
        }
    }
    pub fn valid(&self, admission: &Admission) -> bool {
        self.format_version == 1
            && self.job_id == admission.job_id
            && self.run_id == admission.run_id
            && self.request_sha256 == admission.request_sha256
            && phase(&self.state)
            && self.dispatched_at.as_deref().is_none_or(utc)
            && self.finished_at.as_deref().is_none_or(utc)
            && self.attempt_id.as_deref().is_none_or(attempt)
            && self.result_sha256.as_deref().is_none_or(hash)
            && self.events.iter().all(|event| utc(&event.at))
            && self.dispatched_at.is_some() == self.attempt_id.is_some()
            && match self.state.as_str() {
                "queued" => {
                    self.dispatched_at.is_none()
                        && self.last_sequence == 1
                        && self.events.len() == 1
                        && self.events[0].kind == "queued"
                        && self.finished_at.is_none()
                        && self.result_sha256.is_none()
                        && self.execution_status.is_none()
                        && !self.cancel_requested
                        && self.effect_outcome == "not_attempted"
                }
                "running" | "cancelling" => {
                    self.dispatched_at.is_some()
                        && self.finished_at.is_none()
                        && self.result_sha256.is_none()
                        && self.execution_status.is_none()
                        && self.effect_outcome == "unknown"
                        && (self.state != "cancelling" || self.cancel_requested)
                }
                "finalizing" | "terminal" => {
                    self.result_sha256.is_some()
                        && self.execution_status.is_some()
                        && (self.state == "terminal") == self.finished_at.is_some()
                        && (self.dispatched_at.is_some()
                            || (self.cancel_requested
                                && self.execution_status.as_deref() == Some("cancelled")
                                && self.effect_outcome == "not_attempted"))
                }
                "recovery_required" => {
                    (self.dispatched_at.is_some()
                        || (self.cancel_requested && self.result_sha256.is_some()))
                        && self.finished_at.is_none()
                        && self.execution_status.as_deref() == Some("unknown")
                        && self.effect_outcome == "unknown"
                }
                _ => false,
            }
            && self.last_sequence > 0
            && self.last_sequence < 9_007_199_254_740_991
            && !self.events.is_empty()
            && self.events.len() <= admission.retain_events
            && self
                .events
                .last()
                .is_some_and(|e| e.sequence == self.last_sequence && e.state == self.state)
            && self
                .events
                .iter()
                .all(|event| (1..=self.last_sequence).contains(&event.sequence))
            && self
                .events
                .windows(2)
                .all(|events| events[0].sequence.checked_add(1) == Some(events[1].sequence))
            && self.events.iter().all(|e| {
                phase(&e.state)
                    && matches!(
                        e.kind.as_str(),
                        "queued"
                            | "running"
                            | "heartbeat"
                            | "cancel_requested"
                            | "finalizing"
                            | "terminal"
                            | "recovery_required"
                    )
            })
            && self.execution_status.as_deref().is_none_or(status)
            && matches!(
                self.effect_outcome.as_str(),
                "not_attempted" | "not_applicable" | "succeeded" | "failed" | "unknown"
            )
            && (self.state != "terminal"
                || (self.result_sha256.is_some()
                    && self.finished_at.is_some()
                    && self.execution_status.is_some()))
    }
}
