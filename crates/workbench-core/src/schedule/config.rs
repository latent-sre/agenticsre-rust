use super::*;
pub(super) fn id(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() <= 64 && crate::request::valid_operation(s))
}
pub(super) fn definition(v: &Value) -> Result<Value, Fault> {
    if !keys(
        v,
        &[
            "enabled",
            "timezone",
            "cadence",
            "collection",
            "misfire_grace_ms",
            "retention_limit",
        ],
    ) || !v["enabled"].is_boolean()
        || !v["timezone"].as_str().is_some_and(|s| s.len() <= 128)
        || !v["misfire_grace_ms"]
            .as_u64()
            .is_some_and(|n| (1..=60000).contains(&n))
        || !v["retention_limit"]
            .as_u64()
            .is_some_and(|n| (1..=128).contains(&n))
    {
        return Err(Fault::new("invalid_schedule_definition"));
    }
    calendar::validate(&v["cadence"], v["timezone"].as_str().unwrap())
        .map_err(|_| Fault::new("invalid_schedule_cadence"))?;
    let collection = &v["collection"];
    if !keys(
        collection,
        &[
            "targets",
            "task",
            "concurrency",
            "failure_threshold",
            "timeout_ms",
        ],
    ) || !collection["timeout_ms"]
        .as_u64()
        .is_some_and(|n| (1..=300000).contains(&n))
        || !collection["concurrency"]
            .as_u64()
            .is_some_and(|n| (1..=4).contains(&n))
        || !collection["failure_threshold"]
            .as_u64()
            .is_some_and(|n| (1..=16).contains(&n))
    {
        return Err(Fault::new("invalid_schedule_collection"));
    }
    let a = collection["targets"]
        .as_array()
        .ok_or(Fault::new("invalid_schedule_selection"))?;
    let mut unique = std::collections::BTreeSet::new();
    if a.is_empty()
        || a.len() > 16
        || a.iter().any(|v| {
            !v.as_str().is_some_and(crate::request::valid_operation)
                || !unique.insert(v.as_str().unwrap())
        })
    {
        return Err(Fault::new("invalid_schedule_selection"));
    }
    let mut normalized = v.clone();
    normalized["collection"]["task"] = crate::tasks::normalize_fleet_task(&collection["task"])
        .map_err(|_| Fault::new("invalid_schedule_task"))?;
    Ok(normalized)
}
pub(super) fn input(r: &Request) -> Result<Value, Fault> {
    let v = &r.inputs;
    let valid = match r.operation.as_str() {
        "schedule.create" => keys(v, &["id", "definition"]) && id(&v["id"]),
        "schedule.update" => {
            keys(v, &["id", "expected_revision", "definition"]) && id(&v["id"]) && revision(v)
        }
        "schedule.pause" | "schedule.cancel-active" => {
            keys(v, &["id", "expected_revision"]) && id(&v["id"]) && revision(v)
        }
        "schedule.run-now" => {
            keys(v, &["id", "expected_revision", "request_id"])
                && id(&v["id"])
                && revision(v)
                && v["request_id"]
                    .as_str()
                    .is_some_and(|s| crate::request::bounded(s, 1, 128))
        }
        "schedule.status" => keys(v, &["id"]) && id(&v["id"]),
        "schedule.preview" => {
            keys(v, &["id", "count"])
                && id(&v["id"])
                && v["count"].as_u64().is_some_and(|n| (1..=32).contains(&n))
        }
        "schedule.list" => keys(v, &[]),
        "schedule.work" => {
            keys(v, &["max_occurrences"])
                && v["max_occurrences"]
                    .as_u64()
                    .is_some_and(|n| (1..=16).contains(&n))
        }
        _ => false,
    };
    if !valid {
        return Err(Fault::new("invalid_schedule_input"));
    }
    let mut normalized = v.clone();
    if matches!(r.operation.as_str(), "schedule.create" | "schedule.update") {
        normalized["definition"] = definition(&v["definition"])?;
    }
    Ok(normalized)
}
fn revision(v: &Value) -> bool {
    v["expected_revision"]
        .as_u64()
        .is_some_and(|n| (1..9_007_199_254_740_991).contains(&n))
}
pub(super) fn fleet_input(def: &Value, request_id: &str) -> Value {
    let c = &def["collection"];
    json!({"request_id":request_id,"targets":c["targets"],"task":c["task"],"concurrency":c["concurrency"],"failure_threshold":c["failure_threshold"]})
}
