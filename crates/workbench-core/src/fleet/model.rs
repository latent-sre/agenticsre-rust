use super::*;
use config::Entry;
use serde::{Deserialize, Serialize};
pub(super) const MAX_RECEIPT: usize = 24 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Target {
    pub entry: Entry,
    pub request_id: String,
    pub profile: Option<Value>,
    pub policy_sha256: Option<String>,
    pub max_runtime_ms: Option<u64>,
    pub preflight_error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Admission {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler_binding_sha256: Option<String>,
    pub format_version: u64,
    pub id: String,
    pub request_id: String,
    pub owner_uid: u32,
    pub request_sha256: String,
    pub intent_sha256: String,
    pub inventory_sha256: String,
    pub inventory_revision: String,
    pub selection_sha256: String,
    pub task: Value,
    pub task_binding_sha256: String,
    pub created_at: String,
    pub expires_at: String,
    pub timeout_ms: u64,
    pub concurrency: usize,
    pub failure_threshold: usize,
    pub ordinal: u64,
    pub targets: Vec<Target>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    pub at: String,
    pub admission: String,
    pub remote_state: Option<String>,
    pub job_id: Option<String>,
    pub attempt_id: Option<String>,
    pub result_sha256: Option<String>,
    pub receipt: Option<Value>,
    pub receipt_sha256: String,
    pub receipt_omitted: bool,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TargetState {
    pub phase: String,
    pub outcome: Option<String>,
    pub reason: Option<String>,
    pub submit: Option<Value>,
    pub observation: Option<Observation>,
    pub evidence_sha256: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    pub format_version: u64,
    pub run_id: String,
    pub request_sha256: String,
    pub phase: String,
    pub cancel_requested: bool,
    pub stop_reason: Option<String>,
    pub updated_at: String,
    pub targets: Vec<TargetState>,
}
pub(super) fn uid() -> u32 {
    // SAFETY: geteuid observes the process owner without pointer arguments.
    unsafe { libc::geteuid() }
}
pub(super) fn digest(value: &Value) -> String {
    hash(&serde_json::to_vec(value).unwrap())
}
pub(super) fn valid_hash(v: &str) -> bool {
    v.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub(super) fn valid_id(v: &str) -> bool {
    v.strip_prefix("fleet-")
        .is_some_and(|s| crate::evidence::store::valid_run_id(&format!("run-{s}")))
}
fn utc(v: &str) -> bool {
    v.len() <= 40
        && v.ends_with('Z')
        && time::OffsetDateTime::parse(v, &time::format_description::well_known::Rfc3339)
            .is_ok_and(|t| (1970..=9999).contains(&t.year()))
}
impl Admission {
    pub fn fingerprint(&self) -> String {
        let mut v = serde_json::to_value(self).unwrap();
        v.as_object_mut().unwrap().remove("request_sha256");
        digest(&v)
    }
    pub fn valid(&self, id: &str) -> bool {
        self.scheduler_binding_sha256
            .as_deref()
            .is_none_or(valid_hash)
            && self.format_version == 1
            && self.id == id
            && valid_id(id)
            && self.owner_uid == uid()
            && crate::request::bounded(&self.request_id, 1, 128)
            && self.request_sha256 == self.fingerprint()
            && [
                &self.intent_sha256,
                &self.inventory_sha256,
                &self.selection_sha256,
                &self.task_binding_sha256,
            ]
            .iter()
            .all(|s| valid_hash(s))
            && crate::request::bounded(&self.inventory_revision, 1, 128)
            && utc(&self.created_at)
            && utc(&self.expires_at)
            && (1..=300_000).contains(&self.timeout_ms)
            && (1..=4).contains(&self.concurrency)
            && (1..=16).contains(&self.failure_threshold)
            && self.ordinal < 9_007_199_254_740_991
            && (1..=16).contains(&self.targets.len())
            && self.task["id"] == "error-budget"
            && self.task["version"] == 1
            && self.task["input"].is_object()
            && self.targets.iter().all(|t| {
                config::valid_entry(&t.entry)
                    && crate::request::bounded(&t.request_id, 1, 128)
                    && t.profile.as_ref().is_none_or(Value::is_object)
                    && t.policy_sha256.as_deref().is_none_or(valid_hash)
                    && t.max_runtime_ms
                        .is_none_or(|n| (1..=3_600_000).contains(&n))
                    && (t.preflight_error.is_some()
                        || (t.profile.is_some()
                            && t.policy_sha256.is_some()
                            && t.max_runtime_ms.is_some()))
            })
    }
}
impl State {
    pub fn new(a: &Admission) -> Self {
        Self {
            format_version: 1,
            run_id: a.id.clone(),
            request_sha256: a.request_sha256.clone(),
            phase: "active".into(),
            cancel_requested: false,
            stop_reason: None,
            updated_at: crate::result::timestamp(),
            targets: a
                .targets
                .iter()
                .map(|t| TargetState {
                    phase: if t.preflight_error.is_some() {
                        "finalizing"
                    } else {
                        "queued"
                    }
                    .into(),
                    outcome: t.preflight_error.as_ref().map(|_| "failed".into()),
                    reason: t.preflight_error.clone(),
                    submit: None,
                    observation: None,
                    evidence_sha256: None,
                })
                .collect(),
        }
    }
    pub fn valid(&self, a: &Admission) -> bool {
        self.format_version == 1
            && self.run_id == a.id
            && self.request_sha256 == a.request_sha256
            && matches!(
                self.phase.as_str(),
                "active" | "recovery_required" | "terminal"
            )
            && utc(&self.updated_at)
            && self
                .stop_reason
                .as_deref()
                .is_none_or(|s| crate::request::bounded(s, 1, 256))
            && self.targets.len() == a.targets.len()
            && self.targets.iter().zip(&a.targets).all(|(s, t)| {
                let phase = match s.phase.as_str() {
                    "queued" => {
                        s.submit.is_none()
                            && s.outcome.is_none()
                            && s.evidence_sha256.is_none()
                            && s.observation.is_none()
                    }
                    "intent" | "admitted" => {
                        s.submit.is_some()
                            && (s.outcome.is_none() || s.outcome.as_deref() == Some("unknown"))
                            && s.evidence_sha256.is_none()
                    }
                    "finalizing" => matches!(s.outcome.as_deref(), Some("succeeded" | "failed")),
                    "terminal" => {
                        matches!(s.outcome.as_deref(), Some("succeeded" | "failed"))
                            && s.evidence_sha256.is_some()
                    }
                    "skipped" => {
                        s.submit.is_none()
                            && s.outcome.as_deref() == Some("skipped")
                            && s.observation.is_none()
                            && s.evidence_sha256.is_none()
                    }
                    _ => false,
                };
                phase
                    && s.reason
                        .as_deref()
                        .is_none_or(|s| crate::request::bounded(s, 1, 256))
                    && s.evidence_sha256.as_deref().is_none_or(valid_hash)
                    && s.submit.as_ref().is_none_or(|v| {
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
                        ) && v["retain_events"] == 32
                            && v["request_id"] == t.request_id
                            && v["target"] == t.entry.target
                            && v["operation"] == "task.run"
                            && v["operation_version"] == 1
                            && v["inputs"] == a.task
                            && v["max_runtime_ms"]
                                .as_u64()
                                .is_some_and(|n| (1..=a.timeout_ms).contains(&n))
                    })
                    && s.observation.as_ref().is_none_or(|o| {
                        utc(&o.at)
                            && matches!(
                                o.admission.as_str(),
                                "accepted" | "not_found" | "not_requested" | "unknown"
                            )
                            && o.remote_state.as_deref().is_none_or(|s| {
                                matches!(
                                    s,
                                    "queued"
                                        | "running"
                                        | "cancelling"
                                        | "finalizing"
                                        | "terminal"
                                        | "recovery_required"
                                )
                            })
                            && o.job_id
                                .as_deref()
                                .is_none_or(|s| crate::request::bounded(s, 1, 128))
                            && o.attempt_id
                                .as_deref()
                                .is_none_or(|s| crate::request::bounded(s, 1, 128))
                            && o.error
                                .as_deref()
                                .is_none_or(|s| crate::request::bounded(s, 1, 128))
                            && o.receipt.is_none() == o.receipt_omitted
                            && valid_hash(&o.receipt_sha256)
                            && o.result_sha256.as_deref().is_none_or(valid_hash)
                            && o.receipt.as_ref().is_none_or(|v| {
                                v.is_object()
                                    && digest(v) == o.receipt_sha256
                                    && serde_json::to_vec(v).unwrap().len() <= MAX_RECEIPT
                            })
                    })
            })
            && (self.phase != "terminal"
                || self
                    .targets
                    .iter()
                    .all(|s| matches!(s.phase.as_str(), "terminal" | "skipped")))
    }
    pub fn counts(&self) -> Value {
        let mut counts = json!({"succeeded":0,"failed":0,"unknown":0,"skipped":0,"pending":0});
        for t in &self.targets {
            let k = if t.phase == "finalizing" {
                "pending"
            } else {
                t.outcome.as_deref().unwrap_or("pending")
            };
            counts[k] = json!(counts[k].as_u64().unwrap() + 1);
        }
        counts
    }
    pub fn aggregate(&self) -> &'static str {
        let counts = self.counts();
        let n = self.targets.len() as u64;
        let succeeded = counts["succeeded"].as_u64().unwrap();
        if succeeded == n {
            "succeeded"
        } else if succeeded > 0 {
            "partial"
        } else if counts["unknown"].as_u64().unwrap() > 0 || counts["pending"].as_u64().unwrap() > 0
        {
            "unknown"
        } else {
            "failed"
        }
    }
}
pub(super) fn evidence(a: &Admission, index: usize, s: &TargetState) -> Value {
    json!({"format_version":1,"run_id":a.id,"target_id":a.targets[index].entry.id,"request_id":a.targets[index].request_id,"outcome":s.outcome,"reason":s.reason,"observation":s.observation})
}
#[cfg(test)]
pub(super) fn fixture() -> Admission {
    let h = format!("sha256:{}", "a".repeat(64));
    let task = crate::tasks::normalize_fleet_task(
        &json!({"id":"error-budget","version":1,"input":{"slo":99.9,"bad_minutes":1}}),
    )
    .unwrap();
    let mut a = Admission {
        scheduler_binding_sha256: None,
        format_version: 1,
        id: crate::result::new_id("fleet"),
        request_id: "fixture-parent".into(),
        owner_uid: uid(),
        request_sha256: String::new(),
        intent_sha256: h.clone(),
        inventory_sha256: h.clone(),
        inventory_revision: "fixture".into(),
        selection_sha256: h.clone(),
        task,
        task_binding_sha256: h.clone(),
        created_at: crate::result::timestamp(),
        expires_at: crate::result::timestamp(),
        timeout_ms: 1000,
        concurrency: 1,
        failure_threshold: 2,
        ordinal: 0,
        targets: vec![Target {
            entry: Entry {
                id: "one".into(),
                runner_id: "runner".into(),
                target: "observation".into(),
                runner_config: "/fixture/client.json".into(),
                principal: "operator".into(),
            },
            request_id: "frozen-child".into(),
            profile: Some(json!({})),
            policy_sha256: Some(h),
            max_runtime_ms: Some(1000),
            preflight_error: None,
        }],
    };
    a.request_sha256 = a.fingerprint();
    a
}
