use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub(super) const SHA: &str = "7bd4c25251342606602c0209404b7bc67e45a34b76ba324bc101430443672480";
const BYTES: &[u8] = include_bytes!("../../resources/runbooks/evidence-review.v1.json");
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Definition {
    pub id: String,
    pub version: u64,
    pub description: String,
    pub effect: String,
    pub input_schema: Value,
    pub limits: Value,
    pub steps: Vec<Step>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Step {
    pub id: String,
    pub capability: String,
    pub version: u64,
    pub dependencies: Vec<String>,
    pub description: String,
    pub expected_evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<Condition>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Condition {
    pub step: String,
    pub pointer: String,
    pub equals: String,
}
pub(super) fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(super) fn identity() -> Value {
    json!({"id":"evidence-review","version":1,"sha256":SHA})
}
pub(super) fn validate(d: &Definition) -> Result<(), &'static str> {
    if d.id != "evidence-review" || d.version != 1 || d.effect != "observe" || d.steps.len() != 6 {
        return Err("runbook_invalid_definition");
    }
    let canonical: Value =
        serde_json::from_slice(BYTES).map_err(|_| "runbook_invalid_definition")?;
    if d.input_schema != canonical["input_schema"] || d.limits != canonical["limits"] {
        return Err("runbook_invalid_definition");
    }
    let mut seen = BTreeSet::new();
    for s in &d.steps {
        if s.version != 1
            || s.description.is_empty()
            || s.expected_evidence.is_empty()
            || !matches!(
                s.capability.as_str(),
                "evidence.show" | "evidence.compare" | "human.checkpoint"
            )
            || s.dependencies.iter().any(|id| !seen.contains(id))
            || !seen.insert(s.id.clone())
        {
            return Err("runbook_invalid_definition");
        }
        if let Some(c) = &s.condition
            && (s.capability != "human.checkpoint"
                || c.step != "compare"
                || c.pointer != "/data/state"
                || !matches!(c.equals.as_str(), "complete" | "partial" | "incompatible")
                || !s.dependencies.contains(&c.step))
        {
            return Err("runbook_invalid_definition");
        }
    }
    // This executor is compiled for these exact reviewed bindings, not a generic interpreter.
    for (i, (id, cap, deps, branch)) in [
        ("before", "evidence.show", vec![], None),
        ("after", "evidence.show", vec![], None),
        ("compare", "evidence.compare", vec!["before", "after"], None),
        (
            "review-complete",
            "human.checkpoint",
            vec!["compare"],
            Some("complete"),
        ),
        (
            "review-partial",
            "human.checkpoint",
            vec!["compare"],
            Some("partial"),
        ),
        (
            "review-incompatible",
            "human.checkpoint",
            vec!["compare"],
            Some("incompatible"),
        ),
    ]
    .iter()
    .enumerate()
    {
        let s = &d.steps[i];
        if s.id != *id
            || s.capability != *cap
            || s.dependencies != *deps
            || s.condition.as_ref().map(|c| c.equals.as_str()) != *branch
        {
            return Err("runbook_invalid_definition");
        }
    }
    Ok(())
}
pub(super) fn installed() -> Result<Definition, &'static str> {
    if hash(BYTES) != SHA {
        return Err("runbook_definition_mismatch");
    }
    let d = serde_json::from_slice(BYTES).map_err(|_| "runbook_invalid_definition")?;
    validate(&d)?;
    Ok(d)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_definition_and_rejected_mutations() {
        let d = installed().unwrap();
        for (index, key, value) in [
            (0, "capability", json!("runbook.run")),
            (0, "dependencies", json!(["compare"])),
            (2, "version", json!(2)),
            (
                3,
                "condition",
                json!({"step":"compare","pointer":"/data/state","equals":true}),
            ),
        ] {
            let mut v = serde_json::to_value(&d).unwrap();
            v["steps"][index][key] = value;
            assert!(
                serde_json::from_value::<Definition>(v)
                    .ok()
                    .and_then(|d| validate(&d).ok())
                    .is_none()
            );
        }
    }
}
