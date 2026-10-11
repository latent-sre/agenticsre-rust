//! Offline metadata for the single reviewed collection pack.
use crate::{
    request::{Problem, parse_bounded_json},
    result::OperationResult,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BYTES: &[u8] = include_bytes!("../../resources/evidence/service-queries.v1.json");
const SHA256: &str = "602806667cfb6b1ec1ce4e321a0205814d57ae30b06f6ca879e9e6e5f7319236";
pub(super) const STEP_IDS: [&str; 2] = ["metrics", "logs"];

pub(super) fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn installed() -> Result<Value, Problem> {
    let sha256 = Sha256::digest(BYTES)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if sha256 != SHA256 {
        return Err(Problem::unsupported(
            "invalid_installed_evidence_pack",
            "The installed evidence pack does not match its reviewed identity.",
        ));
    }
    let mut definition = parse_bounded_json(BYTES, 16)?;
    definition["sha256"] = json!(sha256);
    Ok(definition)
}

pub(super) fn select(id: &str, version: u64, expected: Option<&str>) -> Result<Value, Problem> {
    if id != "service-queries" || version != 1 {
        return Err(Problem::unsupported(
            "unknown_evidence_pack",
            "The requested evidence pack id and version are not installed.",
        ));
    }
    if expected.is_some_and(|value| !digest(value)) {
        return Err(Problem::invalid(
            "invalid_evidence_input",
            "An expected evidence pack digest must be 64 lowercase hexadecimal characters.",
        ));
    }
    let definition = installed()?;
    if expected.is_some_and(|pin| definition["sha256"] != pin) {
        return Err(Problem::invalid(
            "pack_digest_mismatch",
            "The expected evidence pack digest does not match the installed definition.",
        ));
    }
    Ok(definition)
}

pub(crate) fn list() -> Value {
    json!({"packs":[installed().expect("reviewed embedded evidence pack")]})
}

pub(crate) fn describe(inputs: &Value, result: &mut OperationResult) {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Identity {
        id: String,
        version: u64,
    }
    let invalid = || {
        Problem::invalid(
            "invalid_evidence_input",
            "Evidence pack description requires only an installed id and version.",
        )
    };
    let outcome = if inputs.is_object() {
        serde_json::from_value::<Identity>(inputs.clone())
            .map_err(|_| invalid())
            .and_then(|input| select(&input.id, input.version, None))
    } else {
        Err(invalid())
    };
    match outcome {
        Ok(definition) => {
            result.effect_outcome = "not_applicable".into();
            result.data = json!({"pack":definition});
        }
        Err(problem) => result.reject(problem),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queries;

    #[test]
    fn reviewed_pack_pins_exact_installed_query_definitions() {
        let definition = installed().unwrap();
        assert_eq!(definition["id"], "service-queries");
        assert_eq!(definition["version"], 1);
        assert_eq!(definition["steps"].as_array().unwrap().len(), 2);
        for (index, step) in definition["steps"].as_array().unwrap().iter().enumerate() {
            assert_eq!(step["id"], STEP_IDS[index]);
            assert_eq!(step["operation"], "query.run");
            let compiled = queries::compile(
                &json!({
                    "id":step["query"]["id"],"version":step["query"]["version"],
                    "expected_definition_sha256":step["query"]["sha256"],
                    "datasource":"fixture","parameters":{"job":"fixture"},
                    "window":{"kind":"relative","lookback_seconds":300}
                }),
                "2026-10-10T12:00:00Z",
            )
            .unwrap();
            assert_eq!(compiled.definition, step["query"]);
        }
        assert_eq!(
            select("service-queries", 1, Some(&"0".repeat(64)))
                .unwrap_err()
                .code,
            "pack_digest_mismatch"
        );
        assert_eq!(
            select("service-queries", 2, None).unwrap_err().code,
            "unknown_evidence_pack"
        );
    }
}
