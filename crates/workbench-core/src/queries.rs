//! Reviewed, immutable query definitions. Caller values enter one quoted label literal only.
use crate::{
    grafana,
    request::{Problem, parse_bounded_json},
    result::OperationResult,
};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

struct Source {
    id: &'static str,
    dialect: &'static str,
    kind: &'static str,
    sha256: &'static str,
    bytes: &'static [u8],
}

const SOURCES: [Source; 2] = [
    Source {
        id: "prometheus.up-by-job",
        dialect: "promql",
        kind: "prometheus",
        sha256: "e42bc20e72b99554cc61b2c09d06b2dd3ad028a1455aa835517f2dd956c1363f",
        bytes: include_bytes!("../resources/queries/prometheus.up-by-job.v1.json"),
    },
    Source {
        id: "loki.logs-by-job",
        dialect: "logql",
        kind: "loki",
        sha256: "d4661dbe0efc261ccb5eb340cbcbceb90568fa2e0973b6518cfa4fd796eb6019",
        bytes: include_bytes!("../resources/queries/loki.logs-by-job.v1.json"),
    },
];

fn digest(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut value = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut value, "{byte:02x}").expect("write to string");
    }
    value
}

fn invalid() -> Problem {
    Problem::invalid(
        "invalid_query_input",
        "Saved queries require an installed id and version, a datasource, one literal job parameter and an explicit bounded time window.",
    )
}

fn installed(source: &Source) -> Result<Value, Problem> {
    let bad_definition = || {
        Problem::unsupported(
            "invalid_installed_query",
            "The installed query definition does not match its reviewed identity.",
        )
    };
    let sha256 = digest(source.bytes);
    let mut definition = parse_bounded_json(source.bytes, 16).map_err(|_| bad_definition())?;
    if sha256 != source.sha256
        || definition["id"].as_str() != Some(source.id)
        || definition["version"].as_u64() != Some(1)
        || definition["dialect"].as_str() != Some(source.dialect)
        || !matches!(source.dialect, "promql" | "logql")
    {
        return Err(bad_definition());
    }
    definition["definition_sha256"] = json!(sha256);
    Ok(definition)
}

fn lookup(id: &str, version: u64) -> Result<&'static Source, Problem> {
    SOURCES
        .iter()
        .find(|source| source.id == id && version == 1)
        .ok_or_else(|| {
            Problem::unsupported(
                "unknown_query_definition",
                "The requested query id and version are not installed.",
            )
        })
}

pub(crate) fn list() -> Value {
    json!({"queries":SOURCES.iter().map(|source| {
        installed(source).expect("reviewed embedded query definition")
    }).collect::<Vec<_>>()})
}

pub(crate) fn describe(inputs: &Value, result: &mut OperationResult) {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Identity {
        id: String,
        version: u64,
    }
    if !inputs.is_object() {
        result.reject(invalid());
        return;
    }
    let definition = serde_json::from_value::<Identity>(inputs.clone())
        .map_err(|_| invalid())
        .and_then(|input| lookup(&input.id, input.version))
        .and_then(installed);
    match definition {
        Ok(definition) => {
            result.effect_outcome = "not_applicable".into();
            result.data = json!({"query":definition});
        }
        Err(problem) => result.reject(problem),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    id: String,
    version: u64,
    datasource: String,
    parameters: Parameters,
    window: Window,
    #[serde(default, deserialize_with = "present_digest")]
    expected_definition_sha256: Option<String>,
}

// Absence is optional; a supplied null is not an absent pin.
fn present_digest<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameters {
    job: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Window {
    Absolute { from: String, to: String },
    Relative { lookback_seconds: u64 },
}

fn millisecond_date(millis: i64) -> Option<(String, i64)> {
    let text = OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000)
        .ok()?
        .format(&Rfc3339)
        .ok()?;
    grafana::date(&text)
}

fn window(window: Window, started_at: &str) -> Result<(String, String), Problem> {
    let (from, to) = match window {
        Window::Absolute { from, to } => (
            grafana::date(&from).ok_or_else(invalid)?,
            grafana::date(&to).ok_or_else(invalid)?,
        ),
        Window::Relative { lookback_seconds } => {
            if !(1..=86400).contains(&lookback_seconds) {
                return Err(invalid());
            }
            // The parent receipt is the only wall-clock sample. Its fractional remainder
            // is discarded once, before deriving either bound or entering the executor.
            let anchor = OffsetDateTime::parse(started_at, &Rfc3339).map_err(|_| invalid())?;
            let nanos = anchor.unix_timestamp_nanos();
            if nanos < 0 {
                return Err(invalid());
            }
            let to = i64::try_from(nanos / 1_000_000).map_err(|_| invalid())?;
            let lookback = i64::try_from(lookback_seconds)
                .ok()
                .and_then(|value| value.checked_mul(1000))
                .ok_or_else(invalid)?;
            let from = to.checked_sub(lookback).ok_or_else(invalid)?;
            (
                millisecond_date(from).ok_or_else(invalid)?,
                millisecond_date(to).ok_or_else(invalid)?,
            )
        }
    };
    if from.1 >= to.1 || to.1 - from.1 > 86_400_000 {
        return Err(invalid());
    }
    Ok((from.0, to.0))
}

/// Resolve once at a parent's original wall-clock anchor. Nested queries receive this
/// absolute form, so their truthful individual start timestamps cannot move the range.
pub(crate) fn resolve_window(value: &Value, started_at: &str) -> Result<Value, Problem> {
    if !value.is_object() {
        return Err(invalid());
    }
    let input = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    let (from, to) = window(input, started_at)?;
    Ok(json!({"kind":"absolute","from":from,"to":to}))
}

// For the admitted control-free Unicode subset, Go's quoted-string grammar needs
// escaping only for the quote and backslash. JSON's optional \u escapes are not used.
pub(crate) fn escaped_label(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(character),
        }
    }
    out
}

// Private values deliberately have no Debug implementation and enter the receipt only
// after the existing credential provider and redactor have been admitted.
pub(crate) struct Compiled {
    pub definition: Value,
    pub job: String,
    pub input: Value,
    pub limitations: Vec<String>,
}

pub(crate) fn compile(inputs: &Value, started_at: &str) -> Result<Compiled, Problem> {
    if !inputs.is_object() || !inputs["parameters"].is_object() || !inputs["window"].is_object() {
        return Err(invalid());
    }
    let input: Input = serde_json::from_value(inputs.clone()).map_err(|_| invalid())?;
    if !(1..=128).contains(&input.parameters.job.len())
        || input.parameters.job.chars().any(char::is_control)
        || input
            .expected_definition_sha256
            .as_ref()
            .is_some_and(|pin| {
                pin.len() != 64
                    || !pin
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
    {
        return Err(invalid());
    }
    let source = lookup(&input.id, input.version)?;
    let definition = installed(source)?;
    if input
        .expected_definition_sha256
        .is_some_and(|pin| definition["definition_sha256"] != pin)
    {
        return Err(Problem::invalid(
            "definition_digest_mismatch",
            "The expected query digest does not match the installed definition.",
        ));
    }
    let (from, to) = window(input.window, started_at)?;
    let quoted_job = format!("\"{}\"", escaped_label(&input.parameters.job));
    let expr = match source.dialect {
        "promql" => format!("up{{job={quoted_job}}}"),
        "logql" => format!("{{job={quoted_job}}}"),
        _ => unreachable!("installed dialect was validated"),
    };
    Ok(Compiled {
        definition: json!({"id":source.id,"version":1,"dialect":source.dialect,"sha256":definition["definition_sha256"]}),
        job: input.parameters.job,
        input: json!({"datasource":input.datasource,"kind":source.kind,"from":from,"to":to,"expr":expr}),
        limitations: definition["limitations"]
            .as_array()
            .expect("reviewed definition limitations")
            .iter()
            .map(|value| value.as_str().expect("reviewed limitation text").into())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::Request;

    const ANCHOR: &str = "2026-10-10T12:34:56.123987654Z";

    fn input() -> Value {
        json!({"id":"prometheus.up-by-job","version":1,"datasource":"metrics-1",
            "parameters":{"job":"fixture"},"window":{"kind":"relative","lookback_seconds":300}})
    }

    #[test]
    fn catalog_digests_pin_exact_installed_bytes_and_identity() {
        let catalog = list();
        assert_eq!(catalog["queries"].as_array().unwrap().len(), 2);
        for source in &SOURCES {
            let metadata = installed(source).unwrap();
            assert_eq!(metadata["definition_sha256"], digest(source.bytes));
            assert_eq!(metadata["id"], source.id);
            assert_eq!(metadata["dialect"], source.dialect);
            assert_eq!(metadata["version"], 1);
            assert_eq!(metadata["parameter_schema"]["additionalProperties"], false);
            assert_eq!(metadata["parameter_schema"]["required"], json!(["job"]));
            assert_eq!(metadata["limits"]["max_range_seconds"], 86400);
            assert_eq!(metadata["limits"]["max_data_points"], 1000);
            let mut request = input();
            request["id"] = json!(source.id);
            request["expected_definition_sha256"] = metadata["definition_sha256"].clone();
            assert!(compile(&request, ANCHOR).is_ok());
            request["expected_definition_sha256"] = json!("0".repeat(64));
            assert_eq!(
                compile(&request, ANCHOR).err().unwrap().code,
                "definition_digest_mismatch"
            );
        }
        let changed = Source {
            bytes: b"{}\n",
            ..SOURCES[0]
        };
        assert_eq!(
            installed(&changed).unwrap_err().code,
            "invalid_installed_query"
        );
        for request in [
            json!({"id":"prometheus.up-by-job","version":1}),
            json!({"id":"loki.logs-by-job","version":1}),
        ] {
            let mut result = OperationResult::new(&Request::new("query.describe", request.clone()));
            describe(&request, &mut result);
            assert!(result.errors.is_empty());
            assert_eq!(result.data["query"]["id"], request["id"]);
        }
    }

    #[test]
    fn compiled_labels_are_single_go_compatible_unicode_literals() {
        let mut request = input();
        request["parameters"]["job"] = json!("quote\" slash\\ snow雪 🐈");
        assert_eq!(
            compile(&request, ANCHOR).unwrap().input["expr"],
            r#"up{job="quote\" slash\\ snow雪 🐈"}"#
        );
        request["id"] = json!("loki.logs-by-job");
        assert_eq!(
            compile(&request, ANCHOR).unwrap().input["expr"],
            r#"{job="quote\" slash\\ snow雪 🐈"}"#
        );
        request["parameters"]["job"] = json!(r#"x"} or up{job="y" | json != `literal`"#);
        assert_eq!(
            compile(&request, ANCHOR).unwrap().input["expr"],
            r#"{job="x\"} or up{job=\"y\" | json != `literal`"}"#
        );
        request["parameters"]["job"] = json!(r"\u0022\x22");
        assert_eq!(
            compile(&request, ANCHOR).unwrap().input["expr"],
            r#"{job="\\u0022\\x22"}"#
        );
        for value in ["a".repeat(128), "é".repeat(64)] {
            request["parameters"]["job"] = json!(value);
            assert!(compile(&request, ANCHOR).is_ok());
        }
        for value in [
            "".into(),
            "a".repeat(129),
            "é".repeat(65),
            "a\u{85}".into(),
            "a\n".into(),
        ] {
            request["parameters"]["job"] = json!(value);
            assert_eq!(
                compile(&request, ANCHOR).err().unwrap().code,
                "invalid_query_input"
            );
        }
    }

    #[test]
    fn one_relative_anchor_and_exact_absolute_normalization() {
        let mut request = input();
        let compiled = compile(&request, ANCHOR).unwrap();
        assert_eq!(compiled.input["from"], "2026-10-10T12:29:56.123Z");
        assert_eq!(compiled.input["to"], "2026-10-10T12:34:56.123Z");
        request["window"] = json!({"kind":"absolute","from":"2026-10-10T07:29:56.12300000000-05:00","to":"2026-10-10T07:34:56.123-05:00"});
        assert_eq!(
            compile(&request, "unused for absolute").unwrap().input,
            compiled.input
        );
        request["window"] = json!({"kind":"relative","lookback_seconds":86400});
        let day = compile(&request, ANCHOR).unwrap();
        assert_eq!(day.input["from"], "2026-10-09T12:34:56.123Z");
        assert_eq!(day.input["to"], "2026-10-10T12:34:56.123Z");
        request["window"] = json!({"kind":"relative","lookback_seconds":1});
        assert_eq!(
            compile(&request, "1970-01-01T00:00:01.000999Z")
                .unwrap()
                .input["from"],
            "1970-01-01T00:00:00Z"
        );
        for anchor in [
            "1970-01-01T00:00:00.999Z",
            "1969-12-31T23:59:59Z",
            "invalid",
        ] {
            assert!(compile(&request, anchor).is_err());
        }
        assert!(compile(&request, "9999-12-31T23:59:59.999999999Z").is_ok());
    }

    #[test]
    fn strict_inputs_reject_null_unknown_types_and_bad_windows() {
        for field in [
            "id",
            "version",
            "datasource",
            "parameters",
            "window",
            "expected_definition_sha256",
        ] {
            let mut request = input();
            request[field] = Value::Null;
            assert_eq!(
                compile(&request, ANCHOR).err().unwrap().code,
                "invalid_query_input"
            );
        }
        let mut request = input();
        request["unexpected"] = json!(true);
        assert!(compile(&request, ANCHOR).is_err());
        for parameters in [
            json!({}),
            json!(["fixture"]),
            json!({"job":"x","region":"y"}),
            json!({"job":null}),
            json!({"job":1}),
        ] {
            request = input();
            request["parameters"] = parameters;
            assert!(compile(&request, ANCHOR).is_err());
        }
        for window in [
            json!({}),
            json!(["relative", 300]),
            json!({"kind":"relative","lookback_seconds":0}),
            json!({"kind":"relative","lookback_seconds":86401}),
            json!({"kind":"relative","lookback_seconds":1.5}),
            json!({"kind":"relative","lookback_seconds":-1}),
            json!({"kind":"relative","lookback_seconds":1,"from":"ignored"}),
            json!({"kind":"relative","lookback_seconds":null}),
            json!({"kind":"relative"}),
            json!({"kind":"absolute","from":"2026-10-10T00:00:00Z","to":"2026-10-10T00:00:00Z"}),
            json!({"kind":"absolute","from":"2026-10-10T00:00:00Z","to":"2026-10-11T00:00:00.001Z"}),
            json!({"kind":"absolute","from":"2026-10-10T00:00:00.0001Z","to":"2026-10-10T00:01:00Z"}),
            json!({"kind":"absolute","from":"2026-10-10T00:00:00Z","to":"2026-10-10T00:01:00Z","lookback_seconds":1}),
            json!({"kind":"absolute","from":null,"to":"2026-10-10T00:01:00Z"}),
            json!({"kind":"now","lookback_seconds":1}),
        ] {
            request = input();
            request["window"] = window;
            assert!(compile(&request, ANCHOR).is_err());
        }
        for pin in [json!("A".repeat(64)), json!("a".repeat(63)), json!(123)] {
            request = input();
            request["expected_definition_sha256"] = pin;
            assert!(compile(&request, ANCHOR).is_err());
        }
        request = input();
        request["version"] = json!(2);
        assert_eq!(
            compile(&request, ANCHOR).err().unwrap().code,
            "unknown_query_definition"
        );
        request = input();
        request["id"] = json!("unknown");
        assert_eq!(
            compile(&request, ANCHOR).err().unwrap().code,
            "unknown_query_definition"
        );
        request = input();
        request["dialect"] = json!("promql");
        assert!(compile(&request, ANCHOR).is_err());
    }
}
