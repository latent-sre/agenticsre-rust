//! Private opt-in workflow metadata; no task arguments, output, people or free text are retained.
#[cfg(target_os = "linux")]
mod execution;
#[cfg(target_os = "linux")]
mod store;
mod summary;
use crate::{
    OperatorContext, RunControl,
    request::{Problem, Request, bounded, valid_operation},
    result::OperationResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Instant;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
const YEAR_MS: u64 = 31_536_000_000;
const MAX_COUNTER: u64 = 1_000_000;
const OUTCOMES: [&str; 8] = [
    "succeeded",
    "failed",
    "partial",
    "cancelled",
    "timed_out",
    "denied",
    "unsupported",
    "unknown",
];
fn bad() -> Problem {
    Problem::invalid(
        "invalid_toil_input",
        "Use only bounded workflow metadata, explicit UTC timestamps and supported toil fields; raw content, person identifiers, unknown fields and explicit-null inputs are not accepted.",
    )
}
fn keys(v: &Value, required: &[&str], optional: &[&str]) -> bool {
    v.as_object().is_some_and(|m| {
        required.iter().all(|k| m.contains_key(*k))
            && m.keys()
                .all(|k| required.contains(&k.as_str()) || optional.contains(&k.as_str()))
            && m.values().all(|v| !v.is_null())
    })
}
fn utc(v: &Value) -> Option<OffsetDateTime> {
    let s = v.as_str()?;
    let b = s.as_bytes();
    if b.len() < 20
        || b.len() > 30
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b.last() != Some(&b'Z')
    {
        return None;
    }
    if ![0..4, 5..7, 8..10, 11..13, 14..16, 17..19]
        .iter()
        .all(|r| b[r.clone()].iter().all(u8::is_ascii_digit))
    {
        return None;
    }
    if b.len() != 20
        && (b[19] != b'.'
            || !(1..=9).contains(&(b.len() - 21))
            || !b[20..b.len() - 1].iter().all(u8::is_ascii_digit))
    {
        return None;
    }
    if b.len() == 20 && b[19] != b'Z' {
        return None;
    }
    if &b[17..19] == b"60" {
        return None;
    } // Do not normalize leap seconds into another instant.
    let t = OffsetDateTime::parse(s, &Rfc3339).ok()?;
    (t.year() >= 1970).then_some(t)
}
fn workflow(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| bounded(s, 1, 64) && valid_operation(s))
}
fn measurement(v: &Value) -> bool {
    matches!(v.as_str(), Some("measured" | "estimated"))
}
fn number(v: &Value, max: u64) -> bool {
    v.as_u64().is_some_and(|n| n <= max)
}
fn sample_id(s: &str) -> bool {
    s.strip_prefix("sample-")
        .is_some_and(|s| crate::evidence::store::valid_run_id(&format!("run-{s}")))
}
fn optional_numbers(v: &Value) -> bool {
    ["elapsed_ms", "effort_ms", "retries", "manual_interventions"]
        .iter()
        .all(|k| {
            v.get(k).is_none_or(|v| {
                number(
                    v,
                    if k.ends_with("_ms") {
                        YEAR_MS
                    } else {
                        MAX_COUNTER
                    },
                )
            })
        })
}
fn validate(operation: &str, input: &Value) -> Result<(), Problem> {
    let valid = match operation {
        "toil.configure" => {
            keys(input, &["enabled", "retention_days"], &[])
                && input["enabled"].is_boolean()
                && input["retention_days"]
                    .as_u64()
                    .is_some_and(|v| (1..=3650).contains(&v))
        }
        "toil.record" => {
            keys(
                input,
                &[
                    "workflow",
                    "cohort",
                    "observed_at",
                    "measurement",
                    "outcome",
                ],
                &["elapsed_ms", "effort_ms", "retries", "manual_interventions"],
            ) && workflow(&input["workflow"])
                && matches!(input["cohort"].as_str(), Some("baseline" | "candidate"))
                && utc(&input["observed_at"]).is_some()
                && measurement(&input["measurement"])
                && input["outcome"]
                    .as_str()
                    .is_some_and(|s| OUTCOMES.contains(&s))
                && optional_numbers(input)
        }
        "toil.capture" => {
            if !keys(input, &["workflow", "cohort", "task"], &[])
                || !workflow(&input["workflow"])
                || !matches!(input["cohort"].as_str(), Some("baseline" | "candidate"))
            {
                false
            } else {
                crate::tasks::validate_installed_input(&input["task"])?;
                true
            }
        }
        "toil.annotate" => {
            keys(
                input,
                &["id", "effort_ms", "measurement"],
                &["retries", "manual_interventions"],
            ) && input["id"].as_str().is_some_and(sample_id)
                && number(&input["effort_ms"], YEAR_MS)
                && measurement(&input["measurement"])
                && optional_numbers(input)
        }
        "toil.prune" => input == &json!({}),
        "toil.delete" => keys(input, &["id"], &[]) && input["id"].as_str().is_some_and(sample_id),
        "toil.summary" => {
            let costs = &input["costs"];
            keys(input, &["workflow", "from", "to", "costs"], &["projection"])
                && workflow(&input["workflow"])
                && utc(&input["from"])
                    .zip(utc(&input["to"]))
                    .is_some_and(|(a, b)| {
                        b > a
                            && (b - a) <= time::Duration::days(3650)
                            && (b - a).whole_nanoseconds() % 1_000_000 == 0
                    })
                && keys(
                    costs,
                    &["build_ms", "maintenance_ms", "incident_ms", "measurement"],
                    &[],
                )
                && ["build_ms", "maintenance_ms", "incident_ms"]
                    .iter()
                    .all(|k| number(&costs[k], YEAR_MS))
                && measurement(&costs["measurement"])
                && input.get("projection").is_none_or(|p| {
                    keys(p, &["runs_per_day", "horizon_days"], &[])
                        && p["runs_per_day"]
                            .as_u64()
                            .is_some_and(|n| (1..=MAX_COUNTER).contains(&n))
                        && p["horizon_days"]
                            .as_u64()
                            .is_some_and(|n| (1..=3650).contains(&n))
                })
        }
        _ => false,
    };
    if valid { Ok(()) } else { Err(bad()) }
}
fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    id: String,
    workflow: String,
    cohort: String,
    observed_at: String,
    #[serde(deserialize_with = "required_option")]
    elapsed_ms: Option<u64>,
    #[serde(deserialize_with = "required_option")]
    elapsed_measurement: Option<String>,
    #[serde(deserialize_with = "required_option")]
    effort_ms: Option<u64>,
    #[serde(deserialize_with = "required_option")]
    effort_measurement: Option<String>,
    #[serde(deserialize_with = "required_option")]
    effort_provenance: Option<String>,
    outcome: String,
    #[serde(deserialize_with = "required_option")]
    retries: Option<u64>,
    #[serde(deserialize_with = "required_option")]
    manual_interventions: Option<u64>,
    source: String,
    #[serde(deserialize_with = "required_option")]
    task: Option<Value>,
    #[serde(deserialize_with = "required_option")]
    operation_run_id: Option<String>,
    #[serde(deserialize_with = "required_option")]
    annotated_at: Option<String>,
}
impl Sample {
    fn valid(&self) -> bool {
        let paired = |ms: Option<u64>, m: &Option<String>| {
            ms.is_none() == m.is_none()
                && ms.is_none_or(|n| n <= YEAR_MS)
                && m.as_ref()
                    .is_none_or(|s| matches!(s.as_str(), "measured" | "estimated"))
        };
        sample_id(&self.id)
            && workflow(&json!(self.workflow))
            && matches!(self.cohort.as_str(), "baseline" | "candidate")
            && utc(&json!(self.observed_at)).is_some()
            && paired(self.elapsed_ms, &self.elapsed_measurement)
            && paired(self.effort_ms, &self.effort_measurement)
            && (self.effort_ms.is_none() == self.effort_provenance.is_none())
            && self
                .effort_provenance
                .as_ref()
                .is_none_or(|v| matches!(v.as_str(), "operator_reported" | "operator_annotated"))
            && OUTCOMES.contains(&self.outcome.as_str())
            && self.retries.is_none_or(|v| v <= MAX_COUNTER)
            && self.manual_interventions.is_none_or(|v| v <= MAX_COUNTER)
            && self
                .annotated_at
                .as_ref()
                .is_none_or(|v| utc(&json!(v)).is_some())
            && match self.source.as_str() {
                "operator_reported" => self.task.is_none() && self.operation_run_id.is_none(),
                "task_capture" => {
                    self.task.as_ref().is_some_and(|v| {
                        keys(v, &["id", "version"], &[])
                            && matches!(
                                v["id"].as_str(),
                                Some("dashboard-hygiene" | "error-budget")
                            )
                            && v["version"] == 1
                    }) && self
                        .operation_run_id
                        .as_ref()
                        .is_some_and(|v| crate::evidence::store::valid_run_id(v))
                        && self.elapsed_ms.is_some()
                        && self.elapsed_measurement.as_deref() == Some("measured")
                }
                _ => false,
            }
    }
}
pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    if let Err(problem) = validate(&request.operation, &request.inputs) {
        result.reject(problem);
        return;
    }
    let Some(path) = &context.toil_state_dir else {
        result.reject(Problem::invalid(
            "toil_store_required",
            "Attach an explicit private --toil-state-dir before accessing opt-in metadata.",
        ));
        return;
    };
    #[cfg(target_os = "linux")]
    execution::run(request, control, path, start, result);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (control, path, start);
        result.reject(Problem::unsupported(
            "unsupported_platform",
            "Private toil metadata is supported on Linux only.",
        ));
    }
}

#[cfg(test)]
mod tests;
