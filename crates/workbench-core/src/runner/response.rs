//! Validate peer receipts against fixed embedded contracts, never against peer-selected schemas.
//! This intentionally implements only the keywords in these pinned schemas and fails closed
//! for additional validation constructs. Reference resolution cannot access files or networks.
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};
fn schemas() -> &'static BTreeMap<String, Value> {
    static VALUE: OnceLock<BTreeMap<String, Value>> = OnceLock::new();
    VALUE.get_or_init(|| {
        [
            include_str!("../../resources/runner/common.schema.json"),
            include_str!("../../resources/runner/change-data.schema.json"),
            include_str!("../../resources/runner/change-plan.schema.json"),
            include_str!("../../resources/runner/prometheus-config-data.schema.json"),
            include_str!("../../resources/runner/dashboard-hygiene-data.schema.json"),
            include_str!("../../resources/runner/error-budget-data.schema.json"),
            include_str!("../../resources/runner/error-budget-input.schema.json"),
            include_str!("../../resources/runner/job-data.schema.json"),
            include_str!("../../resources/runner/result.schema.json"),
            include_str!("../../resources/runner/runner-data.schema.json"),
            include_str!("../../resources/runner/runner-server-config.schema.json"),
            include_str!("../../resources/runner/runner-wire-response.schema.json"),
        ]
        .into_iter()
        .map(|bytes| {
            let v: Value = serde_json::from_str(bytes).expect("embedded schema");
            (v["$id"].as_str().unwrap().into(), v)
        })
        .collect()
    })
}
fn kind(v: &Value, t: &str) -> bool {
    match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "null" => v.is_null(),
        "boolean" => v.is_boolean(),
        "string" => v.is_string(),
        "integer" => v.is_u64() || v.is_i64(),
        "number" => v.is_number(),
        _ => false,
    }
}
fn compare(v: &Value, b: &Value, minimum: bool) -> bool {
    if let (Some(a), Some(b)) = (v.as_u64(), b.as_u64()) {
        if minimum { a >= b } else { a <= b }
    } else {
        let (Some(a), Some(b)) = (v.as_f64(), b.as_f64()) else {
            return false;
        };
        if minimum { a >= b } else { a <= b }
    }
}
fn supported_keywords(schema: &serde_json::Map<String, Value>) -> bool {
    !schema.keys().any(|k| {
        !matches!(
            k.as_str(),
            "$schema"
                | "$id"
                | "$defs"
                | "title"
                | "description"
                | "examples"
                | "default"
                | "$ref"
                | "type"
                | "const"
                | "enum"
                | "allOf"
                | "anyOf"
                | "oneOf"
                | "if"
                | "then"
                | "else"
                | "not"
                | "properties"
                | "required"
                | "dependentRequired"
                | "additionalProperties"
                | "items"
                | "minItems"
                | "maxItems"
                | "uniqueItems"
                | "minLength"
                | "maxLength"
                | "pattern"
                | "format"
                | "minimum"
                | "maximum"
                | "exclusiveMinimum"
                | "exclusiveMaximum"
                | "minProperties"
                | "maxProperties"
        )
    })
}
fn supported_schema(
    s: &Value,
    document: &Value,
    depth: usize,
    references: &mut BTreeSet<(*const Value, *const Value)>,
) -> bool {
    if depth > 96 {
        return false;
    }
    if s.is_boolean() {
        return true;
    }
    let Some(schema) = s.as_object() else {
        return false;
    };
    if !supported_keywords(schema) {
        return false;
    }
    if let Some(reference) = s["$ref"].as_str() {
        let (id, pointer) = reference.split_once('#').unwrap_or((reference, ""));
        let Some(doc) = (if id.is_empty() {
            Some(document)
        } else {
            schemas().get(id)
        }) else {
            return false;
        };
        let Some(target) = (if pointer.is_empty() {
            Some(doc)
        } else {
            doc.pointer(pointer)
        }) else {
            return false;
        };
        // Identity includes the owning document because local references depend on it. A
        // repeated target closes only this reference edge; its siblings still need checking.
        // These pointers are identity keys only and are never dereferenced.
        if references.insert((doc as *const Value, target as *const Value))
            && !supported_schema(target, doc, depth + 1, references)
        {
            return false;
        }
    }
    if s.get("dependentRequired").is_some_and(|dependencies| {
        !dependencies.as_object().is_some_and(|d| {
            d.values().all(|fields| {
                fields
                    .as_array()
                    .is_some_and(|a| a.iter().all(Value::is_string))
            })
        })
    }) {
        return false;
    }
    for name in ["if", "then", "else", "not", "items", "additionalProperties"] {
        if s.get(name)
            .is_some_and(|child| !supported_schema(child, document, depth + 1, references))
        {
            return false;
        }
    }
    for name in ["allOf", "anyOf", "oneOf"] {
        if s.get(name).is_some_and(|children| {
            !children.as_array().is_some_and(|a| {
                a.iter()
                    .all(|child| supported_schema(child, document, depth + 1, references))
            })
        }) {
            return false;
        }
    }
    if s.get("properties").is_some_and(|properties| {
        !properties.as_object().is_some_and(|p| {
            p.values()
                .all(|child| supported_schema(child, document, depth + 1, references))
        })
    }) {
        return false;
    }
    if s.get("format").is_some_and(|format| format != "date-time") {
        return false;
    }
    if let Some(pattern) = s["pattern"].as_str()
        && pattern != r"^(?!.*(?:^|/)\.\.(?:/|$))[A-Za-z0-9][A-Za-z0-9._/-]*$"
        && regex::Regex::new(pattern).is_err()
    {
        return false;
    }
    true
}
fn check(v: &Value, s: &Value, document: &Value, depth: usize) -> bool {
    if depth == 0 && !supported_schema(s, document, 0, &mut BTreeSet::new()) {
        return false;
    }
    if depth > 96 {
        return false;
    }
    if let Some(b) = s.as_bool() {
        return b;
    }
    let Some(schema) = s.as_object() else {
        return false;
    };
    if !supported_keywords(schema) {
        return false;
    }
    let recur = |value: &Value, schema: &Value| check(value, schema, document, depth + 1);
    if let Some(reference) = s["$ref"].as_str() {
        let (id, pointer) = reference.split_once('#').unwrap_or((reference, ""));
        let target = if id.is_empty() {
            Some(document)
        } else {
            schemas().get(id)
        };
        let Some(doc) = target else { return false };
        let Some(target) = (if pointer.is_empty() {
            Some(doc)
        } else {
            doc.pointer(pointer)
        }) else {
            return false;
        };
        if !check(v, target, doc, depth + 1) {
            return false;
        }
    }
    if let Some(t) = s.get("type") {
        let good = if let Some(t) = t.as_str() {
            kind(v, t)
        } else {
            t.as_array()
                .is_some_and(|a| a.iter().any(|t| t.as_str().is_some_and(|t| kind(v, t))))
        };
        if !good {
            return false;
        }
    }
    if s.get("const").is_some_and(|x| x != v)
        || s["enum"].as_array().is_some_and(|a| !a.contains(v))
    {
        return false;
    }
    if s["allOf"]
        .as_array()
        .is_some_and(|a| !a.iter().all(|x| recur(v, x)))
        || s["anyOf"]
            .as_array()
            .is_some_and(|a| !a.iter().any(|x| recur(v, x)))
        || s["oneOf"]
            .as_array()
            .is_some_and(|a| a.iter().filter(|x| recur(v, x)).count() != 1)
    {
        return false;
    }
    if let Some(condition) = s.get("if") {
        let branch = if recur(v, condition) { "then" } else { "else" };
        if s.get(branch).is_some_and(|x| !recur(v, x)) {
            return false;
        }
    }
    if s.get("not").is_some_and(|x| recur(v, x)) {
        return false;
    }
    if let Some(o) = v.as_object() {
        if s["dependentRequired"]
            .as_object()
            .is_some_and(|dependencies| {
                dependencies.iter().any(|(key, required)| {
                    o.contains_key(key)
                        && required.as_array().is_none_or(|fields| {
                            fields
                                .iter()
                                .any(|field| !field.as_str().is_some_and(|f| o.contains_key(f)))
                        })
                })
            })
        {
            return false;
        }

        if s["required"].as_array().is_some_and(|a| {
            a.iter()
                .any(|k| !k.as_str().is_some_and(|k| o.contains_key(k)))
        }) {
            return false;
        }
        if s["minProperties"]
            .as_u64()
            .is_some_and(|n| o.len() < (n as usize))
            || s["maxProperties"]
                .as_u64()
                .is_some_and(|n| o.len() > (n as usize))
        {
            return false;
        }
        for (k, x) in o {
            if let Some(p) = s["properties"].get(k) {
                if !recur(x, p) {
                    return false;
                }
            } else if s["additionalProperties"] == false
                || s.get("additionalProperties")
                    .is_some_and(|p| p.is_object() && !recur(x, p))
            {
                return false;
            }
        }
    }
    if let Some(a) = v.as_array() {
        if s["minItems"]
            .as_u64()
            .is_some_and(|n| a.len() < (n as usize))
            || s["maxItems"]
                .as_u64()
                .is_some_and(|n| a.len() > (n as usize))
        {
            return false;
        }
        if s["uniqueItems"] == true && a.iter().enumerate().any(|(i, x)| a[..i].contains(x)) {
            return false;
        }
        if s.get("items")
            .is_some_and(|i| a.iter().any(|x| !recur(x, i)))
        {
            return false;
        }
    }
    if let Some(t) = v.as_str() {
        let n = t.chars().count();
        if s["minLength"].as_u64().is_some_and(|m| n < (m as usize))
            || s["maxLength"].as_u64().is_some_and(|m| n > (m as usize))
        {
            return false;
        }
        if let Some(pattern) = s["pattern"].as_str() {
            let matches = if pattern == r"^(?!.*(?:^|/)\.\.(?:/|$))[A-Za-z0-9][A-Za-z0-9._/-]*$" {
                !t.split('/').any(|s| s == "..")
                    && regex::Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._/-]*$")
                        .unwrap()
                        .is_match(t)
            } else {
                regex::Regex::new(pattern).is_ok_and(|r| r.is_match(t))
            };
            if !matches {
                return false;
            }
        }
        if let Some(format) = s["format"].as_str()
            && (format != "date-time"
                || time::OffsetDateTime::parse(t, &time::format_description::well_known::Rfc3339)
                    .is_err())
        {
            return false;
        }
    }
    if v.is_number() {
        for (name, min, exclusive) in [
            ("minimum", true, false),
            ("maximum", false, false),
            ("exclusiveMinimum", true, true),
            ("exclusiveMaximum", false, true),
        ] {
            if let Some(b) = s.get(name)
                && (!compare(v, b, min) || (exclusive && v.as_f64() == b.as_f64()))
            {
                return false;
            }
        }
    }
    true
}
pub(super) fn valid(value: &Value) -> bool {
    let schema = &schemas()["urn:sre-workbench:spec:runner-wire-response:0.1"];
    if !check(value, schema, schema, 0) {
        return false;
    }
    if (value["ok"] == true) != value["error"].is_null() {
        return false;
    }
    let d = &value["data"];
    let binding = &d["binding"];
    if !binding.is_null()
        && (binding["scope"]["principal"] != d["principal"]["id"]
            || binding["scope"]["runner_id"] != d["runner"]["id"]
            || binding["scope"]["target"] != d["target"]
            || binding["expires_at"] != d["admission_expires_at"])
    {
        return false;
    }
    let job = &d["job"];
    let child = &d["result"];
    if (d["admission"] == "accepted" && (job.is_null() || binding.is_null()))
        || (!job.is_null() && d["admission"] != "accepted")
    {
        return false;
    }
    if !binding.is_null()
        && (binding["scope"]["grant_sha256"]
            != hash(&serde_json::to_vec(&binding["scope"]["grant"]).unwrap())
            || binding["scope"]["target"]["snapshot_sha256"]
                != hash(&serde_json::to_vec(&binding["scope"]["target"]["snapshot"]).unwrap()))
    {
        return false;
    }

    if !child.is_null()
        && (job.is_null()
            || child["request_id"] != job["request_id"]
            || child["run_id"] != job["run_id"]
            || child["operation"] != job["operation"]
            || child["operation_version"] != job["operation_version"]
            || child["execution"]["status"] != job["execution_status"]
            || child["effect_outcome"] != job["effect_outcome"]
            || child["record_mode"] != "never"
            || child["assessment"] != "not_assessed")
    {
        return false;
    }
    if !job.is_null()
        && (d["request_id"] != job["request_id"] || d["request_sha256"] != job["request_sha256"])
    {
        return false;
    }
    true
}
pub(super) fn budget_data(value: &Value) -> bool {
    let schema = &schemas()["urn:sre-workbench:spec:error-budget-data:0.1"];
    check(value, schema, schema, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_receipt_fields_and_effect_consistency_are_checked() {
        let request = Request::new("task.run", json!({}));
        let r = OperationResult::new(&request);
        let mut value = serde_json::to_value(r).unwrap();
        let schema = &schemas()["urn:sre-workbench:spec:result:0.1"];
        assert!(check(&value, schema, schema, 0));
        value["effect_outcome"] = json!("bogus");
        assert!(!check(&value, schema, schema, 0));
        value["effect_outcome"] = json!("succeeded");
        value["execution"]["status"] = json!("denied");
        assert!(!check(&value, schema, schema, 0));
        value["effect_outcome"] = json!("not_attempted");
        value["errors"] = json!([{"code":"refusal","message":"Denied","retryable":false}]);
        assert!(check(&value, schema, schema, 0));
        value["output"]["stdout_truncated"] = json!("false");
        assert!(!check(&value, schema, schema, 0));
    }
    #[test]
    fn recursive_schema_checks_all_siblings_without_relaxing_value_depth() {
        let schema = json!({
            "type":"object",
            "properties":{"child":{"$ref":"#"}},
            "additionalProperties":false
        });
        assert!(check(&json!({"child":{"child":{}}}), &schema, &schema, 0));
        assert!(!check(
            &json!({"child":{"extra":true}}),
            &schema,
            &schema,
            0
        ));
        let mut too_deep = json!({});
        for _ in 0..100 {
            too_deep = json!({"child":too_deep});
        }
        assert!(!check(&too_deep, &schema, &schema, 0));
        for schema in [
            json!({"anyOf":[{"$ref":"#"},{"unevaluatedProperties":false}]}),
            json!({"$ref":"#","properties":{"unused":{"unevaluatedProperties":false}}}),
            json!({"allOf":[{"$ref":"#"},{"not":{"unevaluatedProperties":false}}]}),
        ] {
            assert!(
                !supported_schema(&schema, &schema, 0, &mut BTreeSet::new()),
                "recursive preflight must inspect unsupported siblings: {schema}"
            );
            assert!(!check(&json!({}), &schema, &schema, 0), "{schema}");
        }
    }
    #[test]
    fn unsupported_schema_constructs_and_external_refs_fail_closed() {
        for schema in [
            json!({"not":{"unevaluatedProperties":false}}),
            json!({"if":{"unevaluatedProperties":false},"else":true}),
            json!({"anyOf":[true,{"unevaluatedProperties":false}]}),
        ] {
            assert!(!check(&json!({}), &schema, &schema, 0), "{schema}");
        }

        assert!(!check(
            &json!({}),
            &json!({"unevaluatedProperties":false}),
            &json!({}),
            0
        ));
        assert!(!check(
            &json!({}),
            &json!({"$ref":"https://example.invalid/schema"}),
            &json!({}),
            0
        ));
    }
    #[test]
    fn embedded_contracts_match_published_schemas() {
        assert_eq!(
            include_str!("../../resources/runner/change-data.schema.json"),
            include_str!("../../../../docs/sre-workbench/schemas/change-data.schema.json"),
            "change-data"
        );
        assert_eq!(
            include_str!("../../resources/runner/change-plan.schema.json"),
            include_str!("../../../../docs/sre-workbench/schemas/change-plan.schema.json"),
            "change-plan"
        );
        assert_eq!(
            include_str!("../../resources/runner/prometheus-config-data.schema.json"),
            include_str!(
                "../../../../docs/sre-workbench/schemas/prometheus-config-data.schema.json"
            ),
            "prometheus-config-data"
        );
        assert_eq!(
            include_str!("../../resources/runner/dashboard-hygiene-data.schema.json"),
            include_str!(
                "../../../../docs/sre-workbench/schemas/dashboard-hygiene-data.schema.json"
            ),
            "dashboard-hygiene-data"
        );
        assert_eq!(
            include_str!("../../resources/runner/common.schema.json"),
            include_str!("../../../../docs/sre-workbench/schemas/common.schema.json"),
            "common"
        );
        assert_eq!(
            include_str!("../../resources/runner/error-budget-data.schema.json"),
            include_str!("../../../../docs/sre-workbench/schemas/error-budget-data.schema.json"),
            "error-budget-data"
        );
        assert_eq!(
            include_str!("../../resources/runner/error-budget-input.schema.json"),
            include_str!("../../../../docs/sre-workbench/schemas/error-budget-input.schema.json"),
            "error-budget-input"
        );
        assert_eq!(
            include_str!("../../resources/runner/job-data.schema.json"),
            include_str!("../../../../docs/sre-workbench/schemas/job-data.schema.json"),
            "job-data"
        );
        assert_eq!(
            include_str!("../../resources/runner/result.schema.json"),
            include_str!("../../../../docs/sre-workbench/schemas/result.schema.json"),
            "result"
        );
        assert_eq!(
            include_str!("../../resources/runner/runner-data.schema.json"),
            include_str!("../../../../docs/sre-workbench/schemas/runner-data.schema.json"),
            "runner-data"
        );
        assert_eq!(
            include_str!("../../resources/runner/runner-server-config.schema.json"),
            include_str!("../../../../docs/sre-workbench/schemas/runner-server-config.schema.json"),
            "runner-server-config"
        );
        assert_eq!(
            include_str!("../../resources/runner/runner-wire-response.schema.json"),
            include_str!("../../../../docs/sre-workbench/schemas/runner-wire-response.schema.json"),
            "runner-wire-response"
        );
    }
    #[test]
    fn real_error_budget_receipt_and_input_dependencies_are_validated() {
        let value: Value = serde_json::from_str(include_str!(
            "../../resources/runner/error-budget-fixture.json"
        ))
        .unwrap();
        assert!(budget_data(&value));
        let schema = &schemas()["urn:sre-workbench:spec:error-budget-input:0.1"];
        let mut input = json!({"slo":99.9,"bad_events":1,"total_events":1000});
        assert!(check(&input, schema, schema, 0));
        input.as_object_mut().unwrap().remove("total_events");
        assert!(!check(&input, schema, schema, 0));
        let mut bad = value;
        bad["input"]["sli_short"] = json!(99.0);
        assert!(!budget_data(&bad));
    }
}
