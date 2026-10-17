use super::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub id: String,
    pub runner_id: String,
    pub target: String,
    pub runner_config: String,
    pub principal: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inventory {
    inventory_version: u64,
    revision: String,
    targets: Vec<Entry>,
}
pub(super) fn load(
    path: &Path,
    c: &RunControl,
    d: Instant,
) -> Result<(String, String, Vec<Entry>), Fault> {
    let bytes = crate::runner::read_private(path, 65536, c, d)
        .map_err(|_| Fault::new("fleet_inventory_unavailable"))?;
    let v = crate::request::parse_bounded_json(&bytes, 8)
        .map_err(|_| Fault::new("invalid_fleet_inventory"))?;
    if !v.is_object()
        || !v["targets"]
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_object))
    {
        return Err(Fault::new("invalid_fleet_inventory"));
    }
    let inv: Inventory =
        serde_json::from_value(v).map_err(|_| Fault::new("invalid_fleet_inventory"))?;
    let mut ids = BTreeSet::new();
    if inv.inventory_version != 1
        || inv.revision.is_empty()
        || inv.revision.len() > 128
        || inv.revision.chars().any(char::is_control)
        || inv.targets.len() > 16
        || inv
            .targets
            .iter()
            .any(|e| !valid_entry(e) || !ids.insert(&e.id))
    {
        return Err(Fault::new("invalid_fleet_inventory"));
    }
    Ok((hash(&bytes), inv.revision, inv.targets))
}
pub(super) fn valid_entry(e: &Entry) -> bool {
    [&e.id, &e.runner_id, &e.target, &e.principal]
        .iter()
        .all(|s| crate::request::valid_operation(s))
        && Path::new(&e.runner_config).is_absolute()
        && e.runner_config.len() <= 1024
        && !e.runner_config.chars().any(char::is_control)
}
pub(super) fn input(request: &Request) -> Result<Value, Fault> {
    let input = &request.inputs;
    let reqid = |v: &Value| {
        v.as_str()
            .is_some_and(|s| crate::request::bounded(s, 1, 128))
    };
    if request.operation == "fleet.collect" {
        if !keys(
            input,
            &[
                "request_id",
                "targets",
                "task",
                "concurrency",
                "failure_threshold",
            ],
        ) || !reqid(&input["request_id"])
            || !input["targets"].as_array().is_some_and(|a| {
                a.len() <= 16
                    && a.iter()
                        .all(|v| v.as_str().is_some_and(crate::request::valid_operation))
            })
            || !input["concurrency"]
                .as_u64()
                .is_some_and(|n| (1..=4).contains(&n))
            || !input["failure_threshold"]
                .as_u64()
                .is_some_and(|n| (1..=16).contains(&n))
        {
            return Err(Fault::new("invalid_fleet_input"));
        }
        let targets = input["targets"].as_array().unwrap();
        if targets.is_empty() {
            return Err(Fault::new("fleet_empty_selection"));
        }
        let mut set = BTreeSet::new();
        if targets.iter().any(|v| !set.insert(v.as_str().unwrap())) {
            return Err(Fault::new("invalid_fleet_input"));
        }
        let mut normalized = input.clone();
        normalized["task"] = crate::tasks::normalize_fleet_task(&input["task"])
            .map_err(|_| Fault::new("invalid_fleet_task"))?;
        Ok(normalized)
    } else if matches!(
        request.operation.as_str(),
        "fleet.status" | "fleet.resume" | "fleet.cancel"
    ) && keys(input, &["request_id"])
        && reqid(&input["request_id"])
    {
        Ok(input.clone())
    } else {
        Err(Fault::new("invalid_fleet_input"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pure_task_normalization_and_admission_refusals() {
        let task = crate::tasks::normalize_fleet_task(
            &json!({"id":"error-budget","version":1,"input":{"slo":99.9,"bad_minutes":1}}),
        )
        .unwrap();
        assert_eq!(task["input"]["window_days"], 28.0);
        assert!(
            crate::tasks::normalize_fleet_task(
                &json!({"id":"error-budget","version":1,"input":[],"command":"forbidden"})
            )
            .is_err()
        );
        let request = Request::new(
            "fleet.collect",
            json!({"request_id":"r","targets":[],"task":task,"concurrency":1,"failure_threshold":1}),
        );
        assert_eq!(input(&request).unwrap_err().code, "fleet_empty_selection");
    }
}
