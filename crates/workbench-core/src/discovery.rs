use serde_json::{Value, json};

pub(crate) const OPERATIONS: [&str; 10] = [
    "process.exec",
    "command.inspect",
    "capability.list",
    "capability.describe",
    "doctor",
    "task.list",
    "task.describe",
    "task.run",
    "grafana.dashboard.get",
    "grafana.query",
];

pub fn capability(operation: &str) -> Option<Value> {
    if !OPERATIONS.contains(&operation) {
        return None;
    }
    let process = matches!(operation, "process.exec" | "command.inspect");
    let grafana = matches!(operation, "grafana.dashboard.get" | "grafana.query");
    let input_schema = if grafana {
        json!({"$ref": if operation == "grafana.query" { "urn:sre-workbench:spec:grafana-query-input:0.1" } else { "urn:sre-workbench:spec:grafana-dashboard-input:0.1" }})
    } else if process {
        json!({"$ref":"urn:sre-workbench:spec:process-input:0.1"})
    } else if operation == "task.run" {
        json!({"$ref":"urn:sre-workbench:spec:task-run-input:0.1"})
    } else if operation == "task.describe" {
        json!({"type":"object", "properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false})
    } else if operation == "capability.describe" {
        json!({"type":"object", "properties":{"operation":{"type":"string"}},"required":["operation"],"additionalProperties":false})
    } else {
        json!({"type":"object","additionalProperties":false})
    };
    Some(json!({
        "spec_version":"0.1", "id":operation, "operation_version":1,
        "title": match operation { "grafana.dashboard.get" => "Read a Grafana dashboard", "grafana.query" => "Query a Grafana datasource", "process.exec" => "Run an operator command", "command.inspect" => "Inspect a command without running it", "capability.list" => "List installed capabilities", "capability.describe" => "Describe an installed capability", "task.run" => "Run a fixed offline task", "task.list" => "List installed tasks", "task.describe" => "Describe an installed task", _ => "Check offline runtime readiness" },
        "description": if grafana { "Native bounded HTTPS using the explicitly configured grafana-legacy-v1 profile and scoped operator environment credential. Fixture evidence only; no live server-version or service-health claim." } else if process { "Literal native commands under the operator account, optionally narrowed by a trusted launcher-attached linux-read-v1 policy. No wire-selected role, protected same-account identity or service health inference." } else if operation == "task.run" { "Run an installed dashboard-hygiene or error-budget version-1 task using fixed embedded source, isolated Python and supervised process limits." } else { "Offline metadata from this installed runtime; no external process or network probe." },
        "effects": if operation == "process.exec" { "unclassified" } else { "observe" },
        "input_schema":input_schema, "output_schema":{"$ref":"urn:sre-workbench:spec:result:0.1"},
        "execution_modes":["foreground"], "platforms":if process || operation == "task.run" || grafana { vec!["linux"] } else { vec!["linux","windows","macos"] },
        "dependencies":if operation == "task.run" { vec![json!({"name":"python3","kind":"interpreter","version_requirement":">=3.11"})] } else { vec![] }, "stability":"experimental", "default_timeout_ms":if grafana {60000} else {30000},"max_timeout_ms":if grafana {60000} else {300000},
        "required_permissions": if operation == "process.exec" { vec!["command.execute"] } else { vec![] },
        "implementation":{"kind":"builtin","reference":format!("workbench-core:{operation}")}
    }))
}

pub fn availability(operation: &str) -> Value {
    if matches!(operation, "grafana.dashboard.get" | "grafana.query") {
        if !cfg!(target_os = "linux") {
            return json!({"status":"unsupported_platform","reason":"Native Grafana fixture acceptance is currently limited to Linux."});
        }
        return json!({"status":"requires_configuration","reason":"Native adapter installed; explicit operator config and a scoped environment credential are required. Discovery does not read secrets or contact a server."});
    }
    if operation == "task.run" {
        return crate::tasks::availability();
    }
    let available =
        cfg!(target_os = "linux") || !matches!(operation, "process.exec" | "command.inspect");
    json!({"status":if available {"available"} else {"unsupported_platform"}, "reason":if available { "Available in the operator-local PoC; invocation inputs are validated before dispatch." } else { "Native process supervision has only been implemented on Linux." }})
}

pub(crate) fn list() -> Value {
    json!({"capabilities": OPERATIONS.iter().map(|operation| json!({"capability":capability(operation),"availability":availability(operation)})).collect::<Vec<_>>()})
}

pub(crate) fn doctor() -> Value {
    json!({"offline":true,"version":env!("CARGO_PKG_VERSION"), "os":std::env::consts::OS,"architecture":std::env::consts::ARCH,
        "execution":availability("process.exec"), "tasks":crate::tasks::list()["tasks"], "grafana":availability("grafana.query"), "record_modes":["never"], "default_timeout_ms":30000,"max_timeout_ms":300000,
        "max_capture_bytes_per_stream":1048576,"max_encoded_result_bytes":2097152,"max_request_bytes":65536,"max_request_depth":16,
        "child_environment":super::process::CHILD_ENV.into_iter().map(|(k,v)| (k.to_owned(), json!(v))).collect::<serde_json::Map<String, Value>>(),
        "stdin":"closed", "target":{"kind":"local","id":"workstation"},"profile":"operator-local",
        "limitations":["No executable version probe or network request is performed.","Process groups are not containment for deliberately escaping programs.","No protected installation, persistence or service health assessment is provided. The optional linux-read-v1 profile requires an explicit trusted policy; this offline check does not establish its kernel availability."]})
}
