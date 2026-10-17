//! Strict browser shapes for the installed definitions; core retains execution semantics.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    id: String,
    version: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Run {
    id: String,
    version: u64,
    datasource: String,
    parameters: Parameters,
    window: Window,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_definition_sha256: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Parameters {
    job: String,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Window {
    Absolute { from: String, to: String },
    Relative { lookback_seconds: u64 },
}

fn installed(id: &str, version: u64) -> bool {
    matches!(id, "prometheus.up-by-job" | "loki.logs-by-job") && version == 1
}

pub(crate) fn normalize(operation: &str, value: Value) -> Result<Value, ()> {
    match operation {
        "queries.list" if value.as_object().is_some_and(|fields| fields.is_empty()) => Ok(value),
        "queries.describe" => {
            let input: Identity = serde_json::from_value(value).map_err(|_| ())?;
            if !installed(&input.id, input.version) {
                return Err(());
            }
            serde_json::to_value(input).map_err(|_| ())
        }
        "query.run" => {
            if !value["parameters"].is_object()
                || !value["window"].is_object()
                || value.get("expected_definition_sha256") == Some(&Value::Null)
            {
                return Err(());
            }
            let input: Run = serde_json::from_value(value).map_err(|_| ())?;
            if !installed(&input.id, input.version)
                || !(1..=40).contains(&input.datasource.len())
                || !input
                    .datasource
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                || !(1..=128).contains(&input.parameters.job.len())
                || input.parameters.job.chars().any(char::is_control)
                || input
                    .expected_definition_sha256
                    .as_ref()
                    .is_some_and(|digest| {
                        digest.len() != 64
                            || !digest
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
                || match &input.window {
                    Window::Absolute { from, to } => {
                        !(1..=64).contains(&from.len()) || !(1..=64).contains(&to.len())
                    }
                    Window::Relative { lookback_seconds } => {
                        !(1..=86400).contains(lookback_seconds)
                    }
                }
            {
                return Err(());
            }
            // Relative time stays unresolved until the core creates its original receipt.
            serde_json::to_value(input).map_err(|_| ())
        }
        _ => Err(()),
    }
}
