use serde::Deserialize;
use serde_json::{Value, json};
use workbench_core::request::Request;

use crate::{grants::Grants, http::ApiError};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submission {
    submission_id: String,
    operation: String,
    operation_version: u64,
    #[serde(default)]
    root_id: Option<String>,
    #[serde(default)]
    target_id: Option<String>,
    inputs: Value,
    limits: Limits,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grants::Options;

    struct Fixture {
        path: std::path::PathBuf,
        grants: Grants,
    }
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(workbench_core::result::new_id("ui-grant"));
            let targets = ["fixture-web", "fixture-other"].map(|id| {
                json!({"id":id,"host":"127.0.0.1","port":9,"path":"/probe",
                    "allowed_ips":["127.0.0.1"],"resolver":{"kind":"system","transport":"udp"}})
            });
            std::fs::write(
                &path,
                serde_json::to_vec(&json!({"schema_version":1,"targets":targets})).unwrap(),
            )
            .unwrap();
            let grants = Grants::new(&Options {
                port: 0,
                allow: vec!["diagnostics.run".into()],
                roots: vec![],
                read_policy: None,
                diagnostics_config: Some(path.clone()),
                targets: vec!["fixture-web".into(), "fixture-other".into()],
                context_source: None,
                context_max_age_days: None,
                evidence_store: None,
                config: None,
                query_targets: vec![],
                collection_targets: vec![],
            })
            .unwrap_or_else(|error| panic!("grant: {error}"));
            Self { path, grants }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_file(&self.path).unwrap();
        }
    }
    fn diagnostic() -> Value {
        json!({"submission_id":"12345678-1234-4234-8234-123456789abc",
            "operation":"diagnostics.run","operation_version":1,"target_id":"fixture-web",
            "inputs":{"id":"connectivity","version":1,"input":{}},
            "limits":{"timeout_ms":30000,"max_output_bytes":1048576}})
    }

    fn query() -> Value {
        json!({"submission_id":"12345678-1234-4234-8234-123456789abc", "operation":"query.run", "operation_version":1,
            "target_id":"query-fixture", "inputs":{"id":"prometheus.up-by-job","version":1,"datasource":"metrics-1",
            "parameters":{"job":"fixture"},"window":{"kind":"relative","lookback_seconds":60}},
            "limits":{"timeout_ms":30000,"max_output_bytes":1048576}})
    }

    #[test]
    fn queries_keep_target_authority_and_retry_identity_separate_from_diagnostics() {
        let mut fixture = Fixture::new();
        fixture.grants.operations.extend([
            "queries.list".into(),
            "queries.describe".into(),
            "query.run".into(),
        ]);
        fixture.grants.query_targets.insert("query-fixture".into());
        let original = normalized(query(), &fixture.grants);
        assert_eq!(original.request.target.kind, "connection");
        assert_eq!(original.request.target.id, "query-fixture");
        assert_eq!(original.request.inputs["window"]["kind"], "relative");
        assert_eq!(
            original.identity,
            normalized(query(), &fixture.grants).identity
        );
        let mut changed = query();
        changed["inputs"]["parameters"]["job"] = json!("different");
        assert_ne!(
            original.identity,
            normalized(changed, &fixture.grants).identity
        );
        let mut wrong_target = query();
        wrong_target["target_id"] = json!("fixture-web");
        assert_eq!(
            normalize(wrong_target, &fixture.grants)
                .err()
                .unwrap()
                .status,
            403
        );
        for (field, value) in [
            ("root_id", Value::Null),
            ("target_id", Value::Null),
            (
                "limits",
                json!({"timeout_ms":30000,"max_output_bytes":1048576,"concurrency":1}),
            ),
            (
                "limits",
                json!({"timeout_ms":30000,"max_output_bytes":1048576,"max_items":100}),
            ),
        ] {
            let mut bad = query();
            bad[field] = value;
            assert_eq!(normalize(bad, &fixture.grants).err().unwrap().status, 422);
        }
        for (field, value) in [
            ("expected_definition_sha256", Value::Null),
            ("expected_definition_sha256", json!("invalid")),
            ("expr", json!("up")),
            ("parameters", json!({"job":"fixture", "expr":"up"})),
            ("parameters", json!(["fixture"])),
            ("window", json!({"kind":"relative","lookback_seconds":0})),
            (
                "window",
                json!({"kind":"relative","lookback_seconds":86401}),
            ),
            (
                "window",
                json!({"kind":"relative","lookback_seconds":60,"from":"now"}),
            ),
            ("version", json!(2)),
            ("id", json!("custom-query")),
        ] {
            let mut bad = query();
            bad["inputs"][field] = value;
            assert_eq!(
                normalize(bad, &fixture.grants).err().unwrap().status,
                422,
                "{field}"
            );
        }
        for operation in ["queries.list", "queries.describe"] {
            let mut catalog = query();
            catalog.as_object_mut().unwrap().remove("target_id");
            catalog["operation"] = json!(operation);
            catalog["inputs"] = if operation == "queries.list" {
                json!({})
            } else {
                json!({"id":"loki.logs-by-job","version":1})
            };
            assert_eq!(
                normalized(catalog.clone(), &fixture.grants)
                    .request
                    .target
                    .kind,
                "local"
            );
            catalog["target_id"] = Value::Null;
            assert_eq!(
                normalize(catalog, &fixture.grants).err().unwrap().status,
                422
            );
        }
    }
    fn normalized(value: Value, grants: &Grants) -> Normalized {
        normalize(value, grants).unwrap_or_else(|error| panic!("HTTP status {}", error.status))
    }

    #[test]
    fn diagnostic_defaults_and_effective_bounds_share_one_retry_identity() {
        let fixture = Fixture::new();
        let default = normalized(diagnostic(), &fixture.grants);
        let mut explicit = diagnostic();
        explicit["inputs"]["input"] = json!({"capture_body":false,"on_failure":"continue"});
        explicit["limits"]["concurrency"] = json!(2);
        explicit["limits"]["max_output_bytes"] = json!(16384);
        let explicit = normalized(explicit, &fixture.grants);
        assert_eq!(default.identity, explicit.identity);
        assert_eq!(default.request.inputs, explicit.request.inputs);
        assert_eq!(default.request.limits.max_output_bytes, 16384);
        assert_eq!(default.request.limits.concurrency, Some(2));
        assert_eq!(default.request.target.kind, "connection");
        assert_eq!(default.request.target.id, "fixture-web");
        assert!(default.root_id.is_none());
        assert_eq!(default.target_id.as_deref(), Some("fixture-web"));
    }

    #[test]
    fn diagnostic_alias_concurrency_and_each_effective_limit_change_retry_identity() {
        let fixture = Fixture::new();
        let original = normalized(diagnostic(), &fixture.grants);
        for (field, value) in [
            ("target_id", json!("fixture-other")),
            (
                "limits",
                json!({"timeout_ms":30000,"max_output_bytes":16384,"concurrency":1}),
            ),
            (
                "limits",
                json!({"timeout_ms":29999,"max_output_bytes":16384}),
            ),
            (
                "limits",
                json!({"timeout_ms":30000,"max_output_bytes":1024}),
            ),
            (
                "inputs",
                json!({"id":"connectivity","version":1,"input":{"capture_body":true}}),
            ),
            (
                "inputs",
                json!({"id":"connectivity","version":1,"input":{"on_failure":"stop"}}),
            ),
        ] {
            let mut changed = diagnostic();
            changed[field] = value;
            assert_ne!(
                original.identity,
                normalized(changed, &fixture.grants).identity
            );
        }
    }

    #[test]
    fn diagnostic_shapes_and_controls_are_strict_before_admission() {
        let fixture = Fixture::new();
        for (field, value) in [
            ("root_id", Value::Null),
            ("root_id", json!("checkout")),
            ("target_id", Value::Null),
            ("target_id", json!(["fixture-web"])),
            (
                "inputs",
                json!({"id":"connectivity","version":1,"input":[false,"continue"]}),
            ),
            (
                "inputs",
                json!({"id":"connectivity","version":1,"input":{"capture_body":null}}),
            ),
            (
                "inputs",
                json!({"id":"connectivity","version":1,"input":{"on_failure":null}}),
            ),
            (
                "inputs",
                json!({"id":"connectivity","version":2,"input":{}}),
            ),
            (
                "inputs",
                json!({"id":"connectivity","version":1,"input":{"pid":1}}),
            ),
            (
                "limits",
                json!({"timeout_ms":60001,"max_output_bytes":16384}),
            ),
            ("limits", json!({"timeout_ms":0,"max_output_bytes":16384})),
            (
                "limits",
                json!({"timeout_ms":30000,"max_output_bytes":2097153}),
            ),
            (
                "limits",
                json!({"timeout_ms":30000,"max_output_bytes":1023}),
            ),
            (
                "limits",
                json!({"timeout_ms":30000,"max_output_bytes":16384,"concurrency":null}),
            ),
            (
                "limits",
                json!({"timeout_ms":30000,"max_output_bytes":16384,"concurrency":3}),
            ),
            (
                "limits",
                json!({"timeout_ms":30000,"max_output_bytes":16384,"concurrency":true}),
            ),
        ] {
            let mut invalid = diagnostic();
            invalid[field] = value;
            assert_eq!(
                normalize(invalid, &fixture.grants).err().unwrap().status,
                422,
                "{field}"
            );
        }
        let mut hidden = diagnostic();
        hidden["target_id"] = json!("hidden");
        assert_eq!(
            normalize(hidden, &fixture.grants).err().unwrap().status,
            403
        );
    }

    #[test]
    fn command_and_task_refuse_diagnostic_selector_and_control_presence_including_null() {
        let mut fixture = Fixture::new();
        fixture.grants.operations.extend([
            "process.exec".into(),
            "command.inspect".into(),
            "task.run".into(),
        ]);
        for operation in ["process.exec", "command.inspect", "task.run"] {
            for (field, value) in [
                ("target_id", Value::Null),
                ("target_id", json!("fixture-web")),
                (
                    "limits",
                    json!({"timeout_ms":30000,"max_output_bytes":16384,"concurrency":null}),
                ),
                (
                    "limits",
                    json!({"timeout_ms":30000,"max_output_bytes":16384,"concurrency":1}),
                ),
            ] {
                let mut invalid = diagnostic();
                invalid.as_object_mut().unwrap().remove("target_id");
                invalid["operation"] = json!(operation);
                invalid[field] = value;
                assert_eq!(
                    normalize(invalid, &fixture.grants).err().unwrap().status,
                    422
                );
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Limits {
    timeout_ms: u64,
    max_output_bytes: usize,
    #[serde(default)]
    concurrency: Option<u64>,
    #[serde(default)]
    max_items: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    program: String,
    args: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Task {
    id: String,
    version: u64,
    input: Value,
}

pub(crate) struct Normalized {
    pub submission_id: String,
    pub request: Request,
    pub root_id: Option<String>,
    pub target_id: Option<String>,
    pub label: &'static str,
    pub identity: [u8; 32],
}

fn uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

pub(crate) fn normalize(value: Value, grants: &Grants) -> Result<Normalized, ApiError> {
    if !value.is_object() || !value["inputs"].is_object() || !value["limits"].is_object() {
        return Err(ApiError::invalid(
            "request",
            "The request, inputs and limits must be JSON objects.",
        ));
    }
    let had_root = value.get("root_id").is_some();
    let had_target = value.get("target_id").is_some();
    let had_max_items = value["limits"].get("max_items").is_some();
    if value["limits"].get("max_items") == Some(&Value::Null) {
        return Err(ApiError::invalid(
            "limits.max_items",
            "max_items must be omitted or an integer from 1 to 1000.",
        ));
    }
    let had_concurrency = value["limits"].get("concurrency").is_some();
    if value["limits"].get("concurrency") == Some(&Value::Null) {
        return Err(ApiError::invalid(
            "limits.concurrency",
            "Concurrency must be omitted or an integer choice, never null.",
        ));
    }
    let input: Submission = serde_json::from_value(value).map_err(|_| {
        ApiError::invalid(
            "request",
            "Missing, unknown or incorrectly typed request fields.",
        )
    })?;
    if !uuid(&input.submission_id) {
        return Err(ApiError::invalid(
            "submission_id",
            "Supply a UUID submission ID.",
        ));
    }
    if !grants.operations.contains(&input.operation) {
        return Err(ApiError::forbidden(
            "operation-not-granted",
            "The launcher did not grant this operation or its dependencies are unavailable.",
        ));
    }
    if input.operation_version != 1 {
        return Err(ApiError::invalid(
            "operation_version",
            "Only operation version 1 is available.",
        ));
    }
    let diagnostic = input.operation == "diagnostics.run";
    let query = input.operation == "query.run";
    let collection = input.operation == "evidence.collect";
    let collection_catalog = matches!(
        input.operation.as_str(),
        "evidence.packs.list" | "evidence.packs.describe"
    );
    let query_catalog = matches!(
        input.operation.as_str(),
        "queries.list" | "queries.describe"
    );
    if (!diagnostic && had_concurrency) || (!diagnostic && !query && !collection && had_target) {
        return Err(ApiError::invalid(
            "request",
            "Only diagnostics, saved queries and collection accept target_id; only diagnostics accept concurrency.",
        ));
    }
    let investigation = grants
        .investigation_grant
        .as_ref()
        .is_some_and(|grant| grant.allows(&input.operation));
    if had_max_items
        && !matches!(
            input.operation.as_str(),
            "evidence.open" | "evidence.compare"
        )
    {
        return Err(ApiError::invalid(
            "limits.max_items",
            "Only evidence.open and evidence.compare accept max_items.",
        ));
    }
    let (inputs, label) = if collection || collection_catalog {
        if had_root {
            return Err(ApiError::invalid(
                "root_id",
                "Evidence collection does not accept a root selector.",
            ));
        }
        if collection {
            let target = input.target_id.as_deref().ok_or_else(|| {
                ApiError::invalid("target_id", "Select one granted collection target.")
            })?;
            if !grants.collection_targets.contains(target) {
                return Err(ApiError::forbidden(
                    "target-not-granted",
                    "Select a collection target explicitly granted by the launcher.",
                ));
            }
            if !(1..=60_000).contains(&input.limits.timeout_ms) {
                return Err(ApiError::invalid(
                    "limits.timeout_ms",
                    "Collection timeout must be 1–60000ms.",
                ));
            }
            let normalized = workbench_core::normalize_evidence_collection_input(&input.inputs)
                .map_err(|_| ApiError::invalid("inputs", "Use the installed service-queries pack schema without paths, expressions or policy overrides."))?;
            (normalized, "Collect evidence")
        } else if input.operation == "evidence.packs.list" && input.inputs == json!({}) {
            (input.inputs, "Evidence pack catalog")
        } else if input.operation == "evidence.packs.describe"
            && input.inputs == json!({"id":"service-queries","version":1})
        {
            (input.inputs, "Evidence pack definition")
        } else {
            return Err(ApiError::invalid(
                "inputs",
                "Select the installed service-queries pack version 1; listing takes empty inputs.",
            ));
        }
    } else if query || query_catalog {
        if had_root {
            return Err(ApiError::invalid(
                "root_id",
                "Saved queries do not accept a root selector, including null.",
            ));
        }
        if query {
            let target = input.target_id.as_deref().ok_or_else(|| {
                ApiError::invalid("target_id", "Select one granted query target.")
            })?;
            if !grants.query_targets.contains(target) {
                return Err(ApiError::forbidden(
                    "target-not-granted",
                    "Select a query target explicitly granted by the launcher.",
                ));
            }
        }
        let inputs = crate::queries::normalize(&input.operation, input.inputs)
            .map_err(|_| ApiError::invalid("inputs", "Use the installed saved-query input schema without expression, endpoint or execution overrides."))?;
        let label = match input.operation.as_str() {
            "queries.list" => "Saved query catalog",
            "queries.describe" => "Saved query definition",
            _ => "Run saved query",
        };
        (inputs, label)
    } else if investigation {
        if had_root || had_target || had_concurrency {
            return Err(ApiError::invalid(
                "request",
                "Offline investigations do not accept root_id, target_id or concurrency, including null.",
            ));
        }
        let normalized =
            workbench_core::normalize_investigation_input(&input.operation, &input.inputs)
                .map_err(|_| {
                    ApiError::invalid(
                        "inputs",
                        "Input does not satisfy the selected offline investigation schema.",
                    )
                })?;
        let label = match input.operation.as_str() {
            "context.resolve" => "Service context",
            "evidence.list" => "Saved evidence",
            "evidence.open" => "Inspect evidence",
            "evidence.compare" => "Compare evidence",
            "report.create" => "Evidence report",
            _ => unreachable!("core investigation grant operation"),
        };
        (normalized, label)
    } else if diagnostic {
        if had_root {
            return Err(ApiError::invalid(
                "root_id",
                "Diagnostics do not accept a command root selector, including null.",
            ));
        }
        let target = input.target_id.as_deref().ok_or_else(|| {
            ApiError::invalid("target_id", "Select exactly one granted diagnostic alias.")
        })?;
        if !grants
            .diagnostic_grant
            .as_ref()
            .is_some_and(|grant| grant.target_ids().any(|id| id == target))
        {
            return Err(ApiError::forbidden(
                "target-not-granted",
                "Select a diagnostic alias explicitly granted by the launcher.",
            ));
        }
        if !(1..=60_000).contains(&input.limits.timeout_ms)
            || !matches!(input.limits.concurrency, None | Some(1 | 2))
        {
            return Err(ApiError::invalid(
                "limits",
                "Diagnostic timeout must be 1–60000ms and concurrency omitted, 1 or 2.",
            ));
        }
        let normalized = workbench_core::normalize_diagnostics_input(&input.inputs).map_err(|_| {
            ApiError::invalid(
                "inputs",
                "Use connectivity version 1 with only boolean capture_body and stop/continue on_failure.",
            )
        })?;
        (normalized, "Connectivity diagnostics")
    } else if input.operation == "task.run" {
        if had_root {
            return Err(ApiError::invalid(
                "root_id",
                "Numerical tasks do not accept a root selector.",
            ));
        }
        let task: Task = serde_json::from_value(input.inputs).map_err(|_| {
            ApiError::invalid(
                "inputs",
                "Supply only task id, version and numerical input.",
            )
        })?;
        if task.id != "error-budget" || task.version != 1 {
            return Err(ApiError::forbidden(
                "task-not-granted",
                "Only error-budget version 1 is granted.",
            ));
        }
        let numerical =
            workbench_core::tasks::normalize_error_budget_input(&task.input).map_err(|_| {
                ApiError::invalid(
                    "inputs.input",
                    "Input does not satisfy the numerical error-budget schema.",
                )
            })?;
        (
            json!({"id":task.id,"version":1,"input":numerical}),
            "Error budget",
        )
    } else {
        let root = input
            .root_id
            .as_ref()
            .and_then(|id| grants.roots.get(id))
            .ok_or_else(|| {
                ApiError::forbidden(
                    "root-not-granted",
                    "Select a root explicitly granted by the launcher.",
                )
            })?;
        let command: Command = serde_json::from_value(input.inputs).map_err(|_| {
            ApiError::invalid(
                "inputs",
                "Supply only the program and literal argument array.",
            )
        })?;
        if !matches!(command.program.as_str(), "git" | "rg")
            || command.args.len() > 64
            || command
                .args
                .iter()
                .any(|arg| arg.chars().count() > 4096 || arg.contains('\0'))
        {
            return Err(ApiError::invalid(
                "inputs",
                "Use git or rg with bounded literal arguments.",
            ));
        }
        let label = match (
            command.program.as_str(),
            command.args.first().map(String::as_str),
        ) {
            ("git", Some("status")) => "Working tree status",
            ("git", Some("diff")) => "Working tree diff",
            ("git", Some("log")) => "Recent commits",
            ("rg", Some("--files")) => "List files",
            ("rg", _) => "Search text",
            _ => "Inspect command",
        };
        (
            json!({"program":command.program,"args":command.args,"cwd":root.path}),
            label,
        )
    };
    let mut request = Request::new(&input.operation, inputs);
    request.operation_version = input.operation_version;
    request.limits.timeout_ms = input.limits.timeout_ms;
    request.limits.max_output_bytes = input.limits.max_output_bytes;
    request.limits.max_items = if matches!(
        input.operation.as_str(),
        "evidence.open" | "evidence.compare"
    ) {
        Some(input.limits.max_items.unwrap_or(100))
    } else {
        None
    };
    if diagnostic {
        request.target.kind = "connection".into();
        request.target.id = input.target_id.clone().expect("admitted diagnostic target");
        request.limits.concurrency = Some(input.limits.concurrency.unwrap_or(2));
    }
    if query || collection {
        request.target.kind = "connection".into();
        request.target.id = input
            .target_id
            .clone()
            .expect("admitted observation target");
    }
    if collection {
        request.record = "always".into();
    }
    request.validate().map_err(|_| {
        ApiError::invalid(
            "limits",
            "Timeout must be 1–300000ms, output 1024–2097152 bytes and max_items 1–1000 when supported.",
        )
    })?;
    if diagnostic {
        request.limits.max_output_bytes = request.limits.max_output_bytes.min(16 * 1024);
    }
    if serde_json::to_vec(&request)
        .expect("finite normalized request")
        .len()
        > workbench_core::request::MAX_REQUEST_BYTES
    {
        return Err(ApiError::invalid(
            "request",
            "The normalized core request exceeds64KiB.",
        ));
    }
    // Exclude the generated core request ID from the deterministic submission identity.
    use sha2::{Digest, Sha256};
    let identity = Sha256::digest(
        serde_json::to_vec(&json!({
            "operation":request.operation,"operation_version":request.operation_version,
            "inputs":request.inputs,"limits":request.limits,"root_id":input.root_id,
            "target_id":input.target_id
        }))
        .expect("finite normalized request"),
    )
    .into();
    Ok(Normalized {
        submission_id: input.submission_id.to_ascii_lowercase(),
        request,
        root_id: input.root_id,
        target_id: input.target_id,
        label,
        identity,
    })
}
