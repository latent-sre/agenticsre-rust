use super::{REVISION, TaskIdentity, digest, invalid_response, required_option, run_fixed_python};
use crate::{
    RunControl,
    request::Problem,
    result::{OperationResult, Status},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::OnceLock;

pub(super) const ID: &str = "error-budget";
const UPSTREAM: &str = include_str!("../../resources/error-budget/upstream.py");
const ADAPTER: &str = include_str!("../../resources/error-budget/calculator.py");
const WRAPPER: &str = include_str!("../../resources/error-budget/wrapper.py");

fn source() -> &'static str {
    static SOURCE: OnceLock<String> = OnceLock::new();
    SOURCE.get_or_init(|| {
        WRAPPER.replace(
            "__WORKBENCH_ADAPTER_SOURCE__",
            &serde_json::to_string(ADAPTER).expect("constant adapter"),
        )
    })
}

pub(super) fn source_identity() -> Value {
    json!({"upstream_revision":REVISION,"upstream_digest":digest(UPSTREAM.as_bytes()),"adapter_digest":digest(ADAPTER.as_bytes()),"binding_digest":digest(source().as_bytes())})
}

pub(super) fn metadata() -> Value {
    json!({"spec_version":"0.1","id":ID,"version":1,"entrypoint":"embedded/error-budget/wrapper.py",
        "digest":digest(source().as_bytes()),"runtime":{"kind":"python","version_requirement":">=3.11; checked inside each invocation"},
        "input_schema":{"$ref":"urn:sre-workbench:spec:error-budget-input:0.1"},"output_schema":{"$ref":"urn:sre-workbench:spec:error-budget-data:0.1"},
        "arguments":[{"input":"input"}],"allowed_environment":["PATH","LANG","LC_ALL","TZ"],"effects":"observe","timeout_ms":30000,"platforms":["linux"]})
}

fn default_days() -> f64 {
    28.0
}
fn default_long() -> String {
    "1h".into()
}
fn default_short() -> String {
    "5m".into()
}
fn non_null_optional<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Input {
    slo: f64,
    #[serde(default = "default_days")]
    window_days: f64,
    #[serde(
        default,
        deserialize_with = "non_null_optional",
        skip_serializing_if = "Option::is_none"
    )]
    bad_minutes: Option<f64>,
    #[serde(
        default,
        deserialize_with = "non_null_optional",
        skip_serializing_if = "Option::is_none"
    )]
    bad_events: Option<f64>,
    #[serde(
        default,
        deserialize_with = "non_null_optional",
        skip_serializing_if = "Option::is_none"
    )]
    total_events: Option<f64>,
    #[serde(
        default,
        deserialize_with = "non_null_optional",
        skip_serializing_if = "Option::is_none"
    )]
    sli_long: Option<f64>,
    #[serde(
        default,
        deserialize_with = "non_null_optional",
        skip_serializing_if = "Option::is_none"
    )]
    sli_short: Option<f64>,
    #[serde(default = "default_long")]
    long_window: String,
    #[serde(default = "default_short")]
    short_window: String,
}

impl Input {
    fn policy(&self) -> Option<(f64, &'static str)> {
        match (self.long_window.as_str(), self.short_window.as_str()) {
            ("1h", "5m") => Some((14.4, "page")),
            ("6h", "30m") => Some((6.0, "page")),
            ("3d", "6h") => Some((1.0, "ticket")),
            _ => None,
        }
    }
    fn valid(&self) -> bool {
        self.slo.is_finite()
            && self.slo > 0.0
            && self.slo < 100.0
            && self.window_days.is_finite()
            && self.window_days > 0.0
            && self.policy().is_some()
            && [self.bad_minutes, self.bad_events]
                .iter()
                .all(|v| v.is_none_or(nonnegative))
            && [self.sli_long, self.sli_short]
                .iter()
                .all(|v| v.is_none_or(|n| nonnegative(n) && n <= 100.0))
            && (self.sli_short.is_none() || self.sli_long.is_some())
            && match (self.bad_events, self.total_events) {
                (None, None) => true,
                (Some(bad), Some(total)) => {
                    self.bad_minutes.is_none() && total.is_finite() && total > 0.0 && bad <= total
                }
                _ => false,
            }
    }
}

fn nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BudgetStatus {
    kind: String,
    unit: String,
    budget: f64,
    consumed: f64,
    remaining: f64,
    consumed_percent: f64,
    state: String,
    #[serde(deserialize_with = "required_option")]
    observed_availability_percent: Option<f64>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Burn {
    policy: String,
    policy_horizon_days: u64,
    long_window: String,
    short_window: String,
    threshold: f64,
    long_rate: f64,
    #[serde(deserialize_with = "required_option")]
    short_rate: Option<f64>,
    severity: String,
    reason: String,
    #[serde(deserialize_with = "required_option")]
    full_budget_projection_days: Option<f64>,
    projection_state: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Calculation {
    allowed_bad_fraction: f64,
    #[serde(deserialize_with = "required_option")]
    status: Option<BudgetStatus>,
    #[serde(deserialize_with = "required_option")]
    burn: Option<Burn>,
}

impl Calculation {
    fn valid(&self, input: &Input) -> bool {
        if !(self.allowed_bad_fraction.is_finite()
            && self.allowed_bad_fraction > 0.0
            && self.allowed_bad_fraction <= 1.0)
        {
            return false;
        }
        if let Some(status) = &self.status {
            let consumed = if status.kind == "time" {
                input.bad_minutes
            } else if status.kind == "request" {
                input.bad_events
            } else {
                return false;
            };
            if consumed != Some(status.consumed)
                || !nonnegative(status.consumed)
                || !nonnegative(status.consumed_percent)
                || !status.budget.is_finite()
                || status.budget <= 0.0
                || !status.remaining.is_finite()
                || !match status.state.as_str() {
                    "ok" => status.remaining > 0.0,
                    "exhausted" => status.remaining == 0.0 && !status.remaining.is_sign_negative(),
                    "over_budget" => status.remaining < 0.0,
                    _ => false,
                }
                || !match status.kind.as_str() {
                    "time" => {
                        status.unit == "minutes" && status.observed_availability_percent.is_none()
                    }
                    "request" => {
                        status.unit == "events"
                            && status
                                .observed_availability_percent
                                .is_some_and(|v| nonnegative(v) && v <= 100.0)
                    }
                    _ => false,
                }
            {
                return false;
            }
        } else if input.bad_minutes.is_some() || input.bad_events.is_some() {
            return false;
        }
        match &self.burn {
            None => input.sli_long.is_none(),
            Some(burn) => {
                let Some((threshold, action)) = input.policy() else {
                    return false;
                };
                if input.sli_long.is_none()
                    || burn.policy != "fixed-30-day-example-v1"
                    || burn.policy_horizon_days != 30
                    || burn.long_window != input.long_window
                    || burn.short_window != input.short_window
                    || burn.threshold != threshold
                    || !nonnegative(burn.long_rate)
                {
                    return false;
                }
                if input.sli_short.is_none() {
                    return burn.short_rate.is_none()
                        && burn.severity == "not_evaluated"
                        && burn.reason == "short_window_missing"
                        && burn.full_budget_projection_days.is_none()
                        && burn.projection_state == "not_evaluated";
                }
                if !burn.short_rate.is_some_and(nonnegative) {
                    return false;
                }
                let decision_valid = match burn.reason.as_str() {
                    "both_windows_at_threshold" => burn.severity == action,
                    "short_window_recovered"
                    | "long_window_unconfirmed"
                    | "both_below_threshold" => burn.severity == "none",
                    _ => false,
                };
                decision_valid
                    && if burn.long_rate == 0.0 {
                        burn.full_budget_projection_days.is_none()
                            && burn.projection_state == "zero_long_burn"
                    } else {
                        burn.full_budget_projection_days
                            .is_some_and(|v| v.is_finite() && v > 0.0)
                            && burn.projection_state == "estimated"
                    }
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    task: TaskIdentity,
    status: String,
    runtime_version: String,
    #[serde(deserialize_with = "required_option")]
    input: Option<Input>,
    #[serde(deserialize_with = "required_option")]
    calculation: Option<Calculation>,
    #[serde(deserialize_with = "required_option")]
    error_code: Option<String>,
}

fn error_message(code: &str) -> Option<&'static str> {
    Some(match code {
        "unsupported_runtime" => {
            "The selected Python runtime is older than the required version 3.11."
        }
        "invalid_calculation_input" => {
            "The supplied calculation fields, numeric ranges or unit/window combinations are invalid."
        }
        "numeric_out_of_range" => {
            "The requested arithmetic exceeds the finite numeric range or underflows a required positive result; no calculation was accepted."
        }
        "task_internal_error" => {
            "The fixed calculator could not complete; no input or exception body is disclosed."
        }
        _ => return None,
    })
}

fn valid_reply(reply: &Reply, expected: &Input) -> bool {
    let Some(supported) = super::runtime_version_supported(&reply.runtime_version) else {
        return false;
    };
    if reply.task.id != ID || reply.task.version != 1 {
        return false;
    }
    match reply.status.as_str() {
        "error" => {
            reply.input.is_none()
                && reply.calculation.is_none()
                && reply
                    .error_code
                    .as_deref()
                    .is_some_and(|code| error_message(code).is_some())
                && (reply.error_code.as_deref() == Some("unsupported_runtime")) != supported
        }
        "complete" => {
            supported
                && reply.error_code.is_none()
                && reply.input.as_ref() == Some(expected)
                && reply
                    .calculation
                    .as_ref()
                    .is_some_and(|c| c.valid(expected))
        }
        _ => false,
    }
}

fn parse_reply(payload: &str, expected: &Input) -> Option<Reply> {
    let value: Value = serde_json::from_str(payload).ok()?;
    if !value.is_object()
        || !value["task"].is_object()
        || [
            "/input",
            "/calculation",
            "/calculation/status",
            "/calculation/burn",
        ]
        .iter()
        .any(|pointer| {
            value
                .pointer(pointer)
                .is_some_and(|v| !v.is_null() && !v.is_object())
        })
    {
        return None;
    }
    serde_json::from_str::<Reply>(payload)
        .ok()
        .filter(|reply| valid_reply(reply, expected))
}

pub(super) fn normalize_input(inputs: &Value) -> Result<Value, Problem> {
    if !inputs.is_object() {
        return Err(Problem::invalid(
            "invalid_task_input",
            "Numerical task input must be a JSON object.",
        ));
    }
    let input = serde_json::from_value::<Input>(inputs.clone()).ok().filter(Input::valid)
        .ok_or_else(|| Problem::invalid("invalid_task_input", "Error-budget requires finite, correctly typed SLO measurements with valid units and a bound window pair."))?;
    Ok(serde_json::to_value(input).expect("finite typed input"))
}

pub(super) fn run(inputs: &Value, control: &RunControl, result: &mut OperationResult) {
    let Some(input) = serde_json::from_value::<Input>(inputs.clone())
        .ok()
        .filter(Input::valid)
    else {
        result.reject(Problem::invalid("invalid_task_input", "Error-budget requires finite, correctly typed SLO measurements with valid units and a bound window pair."));
        return;
    };
    let argument = serde_json::to_string(&input).expect("finite typed input");
    let invocation = run_fixed_python(ID, source(), source_identity(), argument, control, result);
    result.coverage.scope =
        "Arithmetic on supplied SLO measurements and the selected fixed example alert policy only"
            .into();
    result.coverage.limitations.extend([
        "Supplied measurements are not independently verified; no telemetry was collected, no alert was sent and no service health was assessed.".into(),
        "Thresholds are the fixed 30-day example policy; window_days does not rescale them. Crossings use decimal forms of normalized inputs; reported arithmetic is binary64.".into(),
        "One window cannot establish severity. A non-alert does not establish previously consumed budget or an all-clear.".into(),
        "Projection uses the full budget at the long-window rate under steady eligible volume; it is not remaining-budget runway.".into(),
    ]);
    let Some((executable, payload)) = invocation else {
        return;
    };
    let reply = parse_reply(&payload, &input);
    let Some(reply) = reply.filter(|_| payload.len() <= 16 * 1024) else {
        invalid_response(result);
        return;
    };
    if let Some(code) = reply.error_code {
        result.execution.status = if code == "unsupported_runtime" {
            result.effect_outcome = "not_attempted".into();
            Status::Unsupported
        } else {
            Status::Failed
        };
        result.coverage.state = "not_established".into();
        result.error(&code, error_message(&code).expect("validated error code"));
        return;
    }
    result.coverage.state = "complete".into();
    result.data["runtime"] = json!({"executable":executable,"version":reply.runtime_version});
    result.data["input"] = json!(reply.input);
    result.data["calculation"] = json!(reply.calculation);
    result.data["coverage"] = json!({"state":result.coverage.state,"scope":result.coverage.scope,"limitations":result.coverage.limitations});
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_reference_and_adapted_binding_have_separate_identity() {
        assert_eq!(
            digest(UPSTREAM.as_bytes()),
            "sha256:4e55475e6b10a4a2e195b1a6f162d688b093664c06bee69cb09435f1931cf5c2"
        );
        assert_ne!(digest(UPSTREAM.as_bytes()), digest(ADAPTER.as_bytes()));
        assert!(source().len() < 128 * 1024);
    }
    #[test]
    fn finite_numeric_input_survives_json_roundtrip_without_changing_bits() {
        let input: Input = serde_json::from_value(json!({"slo":99.87654321098765,"window_days":1.2345678901234567e290,"bad_events":1.2345678901234566e290,"total_events":1.2345678901234567e290,"sli_long":98.98765432109876,"sli_short":97.87654321098765})).unwrap();
        let encoded = serde_json::to_string(&input).unwrap();
        let decoded: Input = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            input, decoded,
            "finite caller inputs must survive the fixed child protocol roundtrip"
        );
    }
    #[test]
    fn malformed_calculator_replies_and_unrequested_modes_are_rejected() {
        let expected: Input = serde_json::from_value(json!({"slo":99.9})).unwrap();
        let base = json!({"task":{"id":ID,"version":1},"status":"complete","runtime_version":"3.11.2","input":expected,"calculation":{"allowed_bad_fraction":0.001,"status":null,"burn":null},"error_code":null});
        assert!(valid_reply(
            &serde_json::from_value(base.clone()).unwrap(),
            &expected
        ));
        assert!(parse_reply(&base.to_string(), &expected).is_some());
        for (field, value) in [
            ("task", json!([ID, 1])),
            ("input", json!([99.9])),
            ("calculation", json!([0.001, null, null])),
        ] {
            let mut invalid = base.clone();
            invalid[field] = value;
            assert!(
                parse_reply(&invalid.to_string(), &expected).is_none(),
                "{field} must be an object"
            );
        }
        let status_input: Input =
            serde_json::from_value(json!({"slo":99.9,"bad_minutes":0})).unwrap();
        let mut status_reply = base.clone();
        status_reply["input"] = json!(status_input);
        status_reply["calculation"]["status"] =
            json!(["time", "minutes", 40.32, 0, 40.32, 0, "ok", null]);
        assert!(
            valid_reply(
                &serde_json::from_value(status_reply.clone()).unwrap(),
                &status_input
            ),
            "the typed sequence would otherwise be admitted"
        );
        assert!(parse_reply(&status_reply.to_string(), &status_input).is_none());
        let burn_input: Input =
            serde_json::from_value(json!({"slo":99.9,"sli_long":100,"sli_short":100})).unwrap();
        let mut burn_reply = base.clone();
        burn_reply["input"] = json!(burn_input);
        burn_reply["calculation"]["burn"] = json!([
            "fixed-30-day-example-v1",
            30,
            "1h",
            "5m",
            14.4,
            0,
            0,
            "none",
            "both_below_threshold",
            null,
            "zero_long_burn"
        ]);
        assert!(valid_reply(
            &serde_json::from_value(burn_reply.clone()).unwrap(),
            &burn_input
        ));
        assert!(parse_reply(&burn_reply.to_string(), &burn_input).is_none());
        for (field, value) in [
            ("task", json!({"id":"dashboard-hygiene","version":1})),
            ("runtime_version", json!("3.10.9")),
            ("input", json!({"slo":90})),
            (
                "calculation",
                json!({"allowed_bad_fraction":0,"status":null,"burn":null}),
            ),
        ] {
            let mut invalid = base.clone();
            invalid[field] = value;
            assert!(!valid_reply(
                &serde_json::from_value(invalid).unwrap(),
                &expected
            ));
        }
        for field in ["input", "calculation", "error_code"] {
            let mut invalid = base.clone();
            invalid.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<Reply>(invalid).is_err());
        }
    }
}
