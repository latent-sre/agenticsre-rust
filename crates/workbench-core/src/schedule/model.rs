use super::*;
use serde::{Deserialize, Serialize};
pub(super) const TZDB: &str = calendar::TZDB_VERSION;
pub(super) const MAX_SAFE: u64 = 9_007_199_254_740_991;
pub(super) fn uid() -> u32 {
    // SAFETY: geteuid has no pointer arguments.
    unsafe { libc::geteuid() }
}
pub(super) fn utc(s: &str) -> bool {
    s.len() <= 40
        && s.ends_with('Z')
        && time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
            .is_ok_and(|t| (1970..=9999).contains(&t.year()))
}
pub(super) fn digest(s: &str) -> bool {
    s.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Generation {
    pub number: u64,
    pub definition: Value,
    pub tzdb_version: String,
    pub binding: Value,
    pub binding_sha256: String,
    pub fleet_store: String,
    pub inventory: String,
}
impl Generation {
    pub fn fingerprint(&self) -> String {
        hash(
            &json!({"number":self.number,"definition":self.definition,"tzdb_version":self.tzdb_version,"binding":self.binding,"fleet_store":self.fleet_store,"inventory":self.inventory}),
        )
    }
    fn valid(&self) -> bool {
        self.number > 0
            && self.number < MAX_SAFE
            && config::definition(&self.definition).is_ok_and(|v| v == self.definition)
            && self.tzdb_version.len() <= 32
            && self.binding_sha256 == self.fingerprint()
            && self.binding.is_object()
            && self.binding["store"].is_object()
            && self.binding["selection"].is_object()
            && [&self.fleet_store, &self.inventory].iter().all(|s| {
                std::path::Path::new(s).is_absolute()
                    && s.len() <= 4096
                    && !s.chars().any(char::is_control)
            })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Skip {
    pub from_ordinal: i64,
    pub to_ordinal: i64,
    pub count: u64,
    pub reason: String,
    pub at: String,
    pub generation: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Occurrence {
    pub id: String,
    pub last_work_sequence: u64,
    pub schedule_id: String,
    pub generation: u64,
    pub trigger: String,
    pub manual_request_id: Option<String>,
    pub request_sha256: Option<String>,
    pub ordinal: Option<i64>,
    pub due_at: String,
    pub local: Option<String>,
    pub state: String,
    pub reason: Option<String>,
    pub fleet_request_id: String,
    pub effective_timeout_ms: Option<u64>,
    pub fleet_run_id: Option<String>,
    pub fleet_aggregate: Option<String>,
    pub fleet_effect_outcome: Option<String>,
    pub receipt_sha256: Option<String>,
    pub observation_sha256: Option<String>,
    pub receipt: Option<Value>,
    pub receipt_omitted: bool,
    pub cancel_requested: bool,
    pub created_at: String,
    pub updated_at: String,
}
impl Occurrence {
    pub fn unresolved(&self) -> bool {
        matches!(self.state.as_str(), "intent" | "unknown")
    }
    pub fn resolved(&self) -> bool {
        matches!(self.state.as_str(), "terminal" | "skipped" | "failed")
    }
    pub fn observation_fingerprint(&self) -> String {
        hash(
            &json!({"id":self.id,"generation":self.generation,"fleet_request_id":self.fleet_request_id,"state":self.state,"reason":self.reason,"fleet_run_id":self.fleet_run_id,"fleet_aggregate":self.fleet_aggregate,"fleet_effect_outcome":self.fleet_effect_outcome,"receipt_sha256":self.receipt_sha256}),
        )
    }
    fn observation_valid(&self) -> bool {
        if matches!(self.state.as_str(), "unknown" | "terminal" | "failed")
            && self.observation_sha256.as_ref() != Some(&self.observation_fingerprint())
        {
            return false;
        }
        if let Some(v) = &self.receipt {
            let data = &v["data"];
            if self.state == "terminal"
                && (data["run"]["state"] != "terminal"
                    || data["run"]["request_id"] != self.fleet_request_id)
            {
                return false;
            }
            if !data["run"].is_null()
                && data["run"].is_object()
                && (data["run"]["request_id"] != self.fleet_request_id
                    || data["run"]["id"].as_str() != self.fleet_run_id.as_deref()
                    || data["aggregate"].as_str() != self.fleet_aggregate.as_deref()
                    || data["aggregate_effect_outcome"].as_str()
                        != self.fleet_effect_outcome.as_deref())
            {
                return false;
            }
            if self.state == "failed"
                && (v["effect_outcome"] != "not_attempted" || !data["run"].is_null())
            {
                return false;
            }
        }
        true
    }
    fn valid(&self, s: &Schedule) -> bool {
        let generation = s.generations.iter().find(|g| g.number == self.generation);
        self.observation_valid()
            && self.last_work_sequence < MAX_SAFE
            && self.schedule_id == s.id
            && generation.is_some()
            && crate::request::bounded(&self.id, 1, 128)
            && self.fleet_request_id == format!("schedule-fleet-{}", &hash(&json!(self.id))[7..])
            && utc(&self.due_at)
            && utc(&self.created_at)
            && utc(&self.updated_at)
            && self
                .reason
                .as_ref()
                .is_none_or(|s| crate::request::bounded(s, 1, 128))
            && self
                .local
                .as_ref()
                .is_none_or(|s| crate::request::bounded(s, 1, 80))
            && match self.trigger.as_str() {
                "manual" => {
                    self.ordinal.is_none()
                        && self
                            .manual_request_id
                            .as_deref()
                            .is_some_and(|s| crate::request::bounded(s, 1, 128))
                        && self.request_sha256.as_deref().is_some_and(digest)
                }
                "due" => {
                    self.id
                        == format!(
                            "schedule-occurrence-{}",
                            &hash(&json!([s.instance_id, s.id, self.generation, self.ordinal]))
                                [7..]
                        )
                        && self
                            .ordinal
                            .is_some_and(|n| (-2..=i64::MAX - 1).contains(&n))
                        && self.manual_request_id.is_none()
                        && self.request_sha256.is_none()
                }
                _ => false,
            }
            && matches!(
                self.state.as_str(),
                "queued" | "intent" | "unknown" | "terminal" | "skipped" | "failed"
            )
            && self.effective_timeout_ms.is_none_or(|n| {
                n > 0
                    && n <= generation.unwrap().definition["collection"]["timeout_ms"]
                        .as_u64()
                        .unwrap()
            })
            && (!matches!(
                self.state.as_str(),
                "intent" | "unknown" | "terminal" | "failed"
            ) || self.effective_timeout_ms.is_some())
            && (!matches!(self.state.as_str(), "queued" | "skipped")
                || self.effective_timeout_ms.is_none())
            && self
                .fleet_run_id
                .as_deref()
                .is_none_or(|s| crate::request::bounded(s, 1, 128))
            && self
                .fleet_aggregate
                .as_deref()
                .is_none_or(|s| matches!(s, "succeeded" | "partial" | "failed" | "unknown"))
            && self.fleet_effect_outcome.as_deref().is_none_or(|s| {
                matches!(
                    s,
                    "not_attempted"
                        | "not_applicable"
                        | "succeeded"
                        | "failed"
                        | "unknown"
                        | "partial"
                )
            })
            && self.receipt_sha256.as_deref().is_none_or(digest)
            && self.receipt.as_ref().is_none_or(|v| {
                v.is_object()
                    && serde_json::to_vec(v).unwrap().len() <= 16384
                    && self.receipt_sha256.as_ref() == Some(&hash(v))
            })
            && (!self.receipt_omitted || self.receipt.is_none())
            && (self.state != "terminal"
                || (self.fleet_run_id.is_some()
                    && self.fleet_aggregate.is_some()
                    && self.receipt_sha256.is_some()))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Schedule {
    pub instance_id: String,
    pub id: String,
    pub owner_uid: u32,
    pub revision: u64,
    pub generation: u64,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
    pub generations: Vec<Generation>,
    pub cursor: i64,
    pub last_skip: Option<Skip>,
    pub occurrences: Vec<Occurrence>,
}
impl Schedule {
    pub fn current(&self) -> &Generation {
        self.generations.last().unwrap()
    }
    pub fn valid(&self) -> bool {
        crate::request::bounded(&self.instance_id, 1, 128)
            && self.instance_id.starts_with("schedule-")
            && config::id(&json!(self.id))
            && self.owner_uid == uid()
            && self.revision >= self.generation
            && self.revision < MAX_SAFE
            && self.generation > 0
            && utc(&self.created_at)
            && utc(&self.updated_at)
            && !self.generations.is_empty()
            && self.generations.len() <= 128
            && self
                .generations
                .iter()
                .enumerate()
                .all(|(i, g)| g.number == i as u64 + 1 && g.valid())
            && self.current().number == self.generation
            && self.cursor >= -2
            && self.occurrences.len() <= 128
            && self.occurrences.len()
                <= self.current().definition["retention_limit"]
                    .as_u64()
                    .unwrap_or(0) as usize
            && {
                let mut ids = std::collections::BTreeSet::new();
                let mut manual = std::collections::BTreeSet::new();
                self.occurrences.iter().all(|o| {
                    o.valid(self)
                        && ids.insert(&o.id)
                        && o.manual_request_id
                            .as_ref()
                            .is_none_or(|id| manual.insert(id))
                })
            }
            && self.occurrences.iter().filter(|o| o.unresolved()).count() <= 1
            && self.last_skip.as_ref().is_none_or(|s| {
                s.from_ordinal <= s.to_ordinal
                    && s.to_ordinal
                        .checked_sub(s.from_ordinal)
                        .and_then(|n| n.checked_add(1))
                        .and_then(|n| u64::try_from(n).ok())
                        == Some(s.count)
                    && s.reason == "misfire"
                    && utc(&s.at)
                    && s.generation > 0
                    && s.generation <= self.generation
            })
    }
    pub fn summary(&self) -> Value {
        json!({"id":self.id,"owner_uid":self.owner_uid,"revision":self.revision,"generation":self.generation,"enabled":self.enabled,"created_at":self.created_at,"updated_at":self.updated_at,"definition":self.current().definition,"tzdb_version":self.current().tzdb_version,"binding_sha256":self.current().binding_sha256,"cursor":self.cursor,"active_occurrences":self.occurrences.iter().filter(|o|o.unresolved()).count(),"total_occurrences":self.occurrences.len(),"last_skip":self.last_skip})
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    pub work_sequence: u64,
    pub format_version: u64,
    pub owner_uid: u32,
    pub schedules: Vec<Schedule>,
}
impl State {
    pub fn new() -> Self {
        Self {
            work_sequence: 0,
            format_version: 1,
            owner_uid: uid(),
            schedules: vec![],
        }
    }
    pub fn valid(&self) -> bool {
        let mut ids = std::collections::BTreeSet::new();
        self.work_sequence < MAX_SAFE
            && self
                .schedules
                .iter()
                .flat_map(|s| &s.occurrences)
                .all(|o| o.last_work_sequence <= self.work_sequence)
            && self.format_version == 1
            && self.owner_uid == uid()
            && self.schedules.len() <= 16
            && self
                .schedules
                .iter()
                .all(|s| s.valid() && ids.insert(&s.id))
    }
}
