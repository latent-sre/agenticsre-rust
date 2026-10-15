use serde_json::{Value, json};

pub(crate) const OPERATIONS: [&str; 75] = [
    "change.plan",
    "change.status",
    "change.approve",
    "change.apply",
    "change.reconcile",
    "schedule.create",
    "schedule.update",
    "schedule.pause",
    "schedule.run-now",
    "schedule.cancel-active",
    "schedule.status",
    "schedule.list",
    "schedule.preview",
    "schedule.work",
    "fleet.collect",
    "fleet.status",
    "fleet.resume",
    "fleet.cancel",
    "runner.handshake",
    "runner.submit",
    "runner.status",
    "runner.watch",
    "runner.cancel",
    "job.submit",
    "job.status",
    "job.watch",
    "job.cancel",
    "job.work",
    "extension.inspect",
    "extension.install",
    "extension.list",
    "toil.configure",
    "toil.record",
    "toil.capture",
    "toil.annotate",
    "toil.summary",
    "toil.prune",
    "toil.delete",
    "config.check",
    "report.create",
    "runbook.list",
    "runbook.show",
    "runbook.run",
    "runbook.resume",
    "runbook.status",
    "evidence.packs.list",
    "evidence.packs.describe",
    "evidence.collect",
    "evidence.compare",
    "evidence.open",
    "evidence.replay",
    "evidence.list",
    "evidence.show",
    "evidence.protect",
    "evidence.unprotect",
    "evidence.delete",
    "evidence.prune",
    "evidence.recover",
    "queries.list",
    "queries.describe",
    "query.run",
    "diagnostics.run",
    "diagnostics.list",
    "diagnostics.describe",
    "context.resolve",
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
    let change = operation.starts_with("change.");
    let schedule = operation.starts_with("schedule.");
    let fleet = operation.starts_with("fleet.");
    let runner = operation.starts_with("runner.");
    let job = operation.starts_with("job.");
    let extension = operation.starts_with("extension.");
    let toil = operation.starts_with("toil.");
    let toil_write = toil && operation != "toil.summary";
    let runbook = operation.starts_with("runbook.");
    let runbook_write = matches!(operation, "runbook.run" | "runbook.resume");
    let process = matches!(operation, "process.exec" | "command.inspect");
    let query = operation == "query.run";
    let grafana = matches!(operation, "grafana.dashboard.get" | "grafana.query") || query;
    let context = operation == "context.resolve";
    let diagnostics = operation == "diagnostics.run";
    let collection = operation == "evidence.collect";
    let evidence_store =
        operation.starts_with("evidence.") && !operation.starts_with("evidence.packs.");
    let evidence_write = evidence_store
        && !matches!(
            operation,
            "evidence.list"
                | "evidence.show"
                | "evidence.compare"
                | "evidence.open"
                | "evidence.replay"
        );
    let input_schema = if change
        || schedule
        || fleet
        || runner
        || job
        || matches!(operation, "extension.inspect" | "extension.install")
    {
        json!({"$ref":format!("urn:sre-workbench:spec:{}-input:0.1",operation.replace('.',"-"))})
    } else if operation == "toil.prune" {
        json!({"type":"object","additionalProperties":false})
    } else if operation == "config.check" {
        json!({"$ref":"urn:sre-workbench:spec:config-check-input:0.1"})
    } else if toil {
        json!({"$ref":format!("urn:sre-workbench:spec:{}-input:0.1",operation.replace('.',"-"))})
    } else if operation == "report.create" {
        json!({"$ref":"urn:sre-workbench:spec:report-create-input:0.1"})
    } else if runbook && operation != "runbook.list" {
        json!({"$ref": format!("urn:sre-workbench:spec:{}-input:0.1", operation.replace('.', "-"))})
    } else if collection {
        json!({"$ref":"urn:sre-workbench:spec:evidence-collect-input:0.1"})
    } else if operation == "evidence.compare" {
        json!({"$ref":"urn:sre-workbench:spec:evidence-compare-input:0.1"})
    } else if operation == "evidence.packs.describe" {
        json!({"type":"object","properties":{"id":{"const":"service-queries"},"version":{"const":1}},"required":["id","version"],"additionalProperties":false})
    } else if matches!(
        operation,
        "evidence.open"
            | "evidence.replay"
            | "evidence.show"
            | "evidence.protect"
            | "evidence.unprotect"
            | "evidence.delete"
    ) {
        json!({"$ref":"urn:sre-workbench:spec:evidence-reference-input:0.1"})
    } else if query {
        json!({"$ref":"urn:sre-workbench:spec:query-run-input:0.1"})
    } else if operation == "queries.describe" {
        json!({"type":"object","properties":{"id":{"type":"string"},"version":{"const":1}},"required":["id","version"],"additionalProperties":false})
    } else if diagnostics {
        json!({"$ref":"urn:sre-workbench:spec:diagnostics-run-input:0.1"})
    } else if operation == "diagnostics.describe" {
        json!({"type":"object","properties":{"id":{"const":"connectivity"}},"required":["id"],"additionalProperties":false})
    } else if context {
        json!({"$ref":"urn:sre-workbench:spec:context-resolve-input:0.1"})
    } else if grafana {
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
        "title": match operation {"change.plan"=>"Validate and retain an exact rule-file change plan", "change.status"=>"Read an authorized retained change plan", "change.approve"=>"Approve an exact change digest as a distinct principal", "change.apply"=>"Apply a once-approved exact rule-file replacement", "change.reconcile"=>"Observe original change identities without replay", "fleet.collect"=>"Collect a frozen set of runner observations", "fleet.status"=>"Read durable fleet state", "fleet.resume"=>"Reconcile original fleet child intents", "fleet.cancel"=>"Request cancellation for a fleet run", "runner.handshake"=>"Authenticate a remote runner", "runner.submit"=>"Admit a remote job", "runner.status"=>"Look up an authenticated remote request", "runner.watch"=>"Reconnect to remote job events", "runner.cancel"=>"Request remote cancellation", "job.submit"=>"Persist an admitted local job", "job.status"=>"Read durable job state and outcome", "job.watch"=>"Reconnect to bounded job events", "job.cancel"=>"Persist cancellation intent", "job.work"=>"Explicit local worker entry", "extension.inspect" => "Inspect an inert extension package", "extension.install" => "Install a pinned disabled extension package", "extension.list" => "Verify the private extension inventory", "config.check" => "Check declared configuration with a pinned installed validator", "toil.configure" => "Configure private metadata consent and retention", "toil.record" => "Record bounded operator workflow metadata", "toil.capture" => "Measure one installed task attempt", "toil.annotate" => "Annotate effort for an existing episode", "toil.summary" => "Compare workflow effort and declared costs", "toil.prune" => "Prune expired metadata", "toil.delete" => "Delete one metadata sample", "report.create" => "Create a historical evidence handoff report", "runbook.list" => "List installed guided runbooks", "runbook.show" => "Show a versioned guided runbook", "runbook.run" => "Start an observation-only guided runbook", "runbook.resume" => "Resume a version-bound runbook checkpoint", "runbook.status" => "Inspect a retained runbook journal", "evidence.packs.list" => "List installed evidence packs", "evidence.packs.describe" => "Describe an installed evidence pack", "evidence.collect" => "Collect and retain saved-query evidence", "evidence.compare" => "Compare retained metric samples and log entries", "evidence.open" => "Open verified historical query evidence", "evidence.replay" => "Analyze retained observations offline", "evidence.list" => "List local evidence runs", "evidence.show" => "Verify and show an evidence manifest", "evidence.protect" => "Protect retained evidence", "evidence.unprotect" => "Remove evidence retention protection", "evidence.delete" => "Delete unprotected owned evidence", "evidence.prune" => "Expire old unprotected sealed evidence", "evidence.recover" => "Recover interrupted evidence without replay", "queries.list" => "List installed saved queries", "queries.describe" => "Describe an installed saved query", "query.run" => "Run an installed saved query", "diagnostics.run" => "Run configured connectivity observations", "diagnostics.list" => "List installed diagnostic packs", "diagnostics.describe" => "Describe a fixed diagnostic pack", "context.resolve" => "Resolve offline service context", "grafana.dashboard.get" => "Read a Grafana dashboard", "grafana.query" => "Query a Grafana datasource", "process.exec" => "Run an operator command", "command.inspect" => "Inspect a command without running it", "capability.list" => "List installed capabilities", "capability.describe" => "Describe an installed capability", "task.run" => "Run a fixed offline task", "task.list" => "List installed tasks", "task.describe" => "Describe an installed task", _ => "Check offline runtime readiness" },
        "description": if change {"Explicit authenticated requester and approver grants, exact plan digest, pinned rule validation, durable one-shot dispatch and inode-based readback. Recovery requires a new approved plan. No service reload or current-health inference."} else if schedule {"Owner-bound versioned observational schedules with pinned IANA timezone rules, exact fleet bindings, durable occurrence intent, explicit foreground worker ownership, deterministic skips and recovery without replay. No daemon, notifications or current-health inference."} else if fleet {"Explicit finite inventory, pinned profiles and runner authority, durable child admission intents, bounded concurrency and no replay. Recovery only observes or cancels original child request IDs. The fixed error-budget consumer calculates supplied measurements; this is not live health assessment."} else if runner {"Authenticated explicit runner lifecycle with independently pinned identities, target grants, durable request-ID deduplication and bounded reconnect observations. No implicit retry, generic command forwarding, or hosted grants."} else if job {"Private durable local jobs with immutable admission fingerprints, explicit worker ownership, separate host-capped runtime, monotonic retained events and no replay after uncertain dispatch. Submission grants no authority; extension jobs require the same exact current package and policy. job.work uses a separate explicit operator worker entry; hosted grants are unavailable."} else if extension { "Strict bounded package and schema admission; verify embedded executable identity without materializing or executing it. Installation requires a raw package digest pin and explicit private store. Package provenance remains unverified. Management never executes code; separate startup policy grants can authorize exact installed capabilities under ordinary operator authority. Hosted grants are unavailable." } else if operation == "config.check" { "Check declared Grafana dashboards with the embedded Python binding, or Prometheus rules with an explicitly configured exact pinned promtool binary. Bind consumed model bytes and validator identity; preserve unsupported coverage, typed findings and untrusted diagnostics. Dependencies are per kind; no arbitrary validator, source mutation, live query, deployment validation or hosted grant." } else if toil { "Private opt-in bounded workflow metadata. Capture runs one installed task, retains no arguments/output, and never infers human effort from runtime. Explicit consent and retention, no silent eviction, exact break-even arithmetic, failure-inclusive cohort summaries, no employee ranking or external telemetry." } else if operation == "report.create" { "Render Markdown or JSON from one to four verified historical bundles under the offline execution profile. Preserve sourced/untrusted labels, missing/redacted observations and disagreements. Operator knowledge suggestions remain pending proposals; no model interpretation, source update, live request or publishing." } else if runbook { "Reviewed evidence observations, typed branches and explicit human checkpoints. Execution requires a separate private journal and explicit recording. Resume checks revisions and immutable inputs; completed observations are never implicitly replayed. No shell, recursive runbook or hosted grant." } else if collection { "Resolve fresh service bindings and execute the installed two-query pack with one deadline and window. Retain finalized redacted receipts in the explicitly configured private store; auto or always is required. Capture completeness does not establish telemetry coverage or health." } else if operation == "evidence.compare" { "Compare two verified local bundles using an explicit exact-window or window-offset policy. Check source/query identity and sampling; preserve missing, incompatible and partial observations. No network, credentials, query replay or health inference. max_items bounds details; max_output_bytes bounds their encoded JSON." } else if matches!(operation, "evidence.open" | "evidence.replay" | "report.create") { "Verify historical evidence under the Linux x86_64 offline execution profile, which disables socket/network and command/process creation syscalls. Open bounded original receipts or run the installed retained-observations-v1 analysis. Preserve observation times, missing and incompatible sources, and separate analysis provenance. No current health inference, config access or hosted grants." } else if evidence_store { "Access only generated references in the explicitly configured private versioned store. Verify retained bytes and preserve unknown formats. Management writes are explicit; no query replay or hosted grant." } else if query { "Run a fixed installed PromQL or LogQL definition with literal typed parameters, an explicit connection/datasource and one resolved time window. Reuses the scoped Grafana query permission; no hosted grant or health assessment." } else if diagnostics { "Fixed DNS/TCP/TLS/HTTP plus exact local process observations using explicit trusted startup configuration. No health inference, wire-selected destination, grant, command, redirect, retry or persistence." } else if context { "Resolve exact service/environment selectors from an explicit offline fixture-export and bounded startup review-age policy. Source declarations remain sourced data; no live catalog, target dispatch, approval or credential authority." } else if grafana { "Native bounded HTTPS using the explicitly configured grafana-legacy-v1 profile and scoped operator environment credential. Fixture evidence only; no live server-version or service-health claim." } else if process { "Literal native commands under the operator account, optionally narrowed by a trusted launcher-attached linux-read-v1 policy. No wire-selected role, protected same-account identity or service health inference." } else if operation == "task.run" { "Run an installed dashboard-hygiene or error-budget version-1 task using fixed embedded source, isolated Python and supervised process limits." } else { "Offline metadata from this installed runtime; no external process or network probe." },
        "effects": if (change && operation != "change.status") || (schedule && !matches!(operation,"schedule.status"|"schedule.list"|"schedule.preview")) || matches!(operation,"fleet.collect"|"fleet.resume"|"fleet.cancel") {"local_write"} else if matches!(operation,"process.exec"|"job.work"|"runner.submit"|"runner.cancel") { "unclassified" } else if matches!(operation,"job.submit"|"job.cancel") || operation == "extension.install" || evidence_write || runbook_write || toil_write { "local_write" } else { "observe" },
        "input_schema":input_schema, "output_schema":{"$ref":"urn:sre-workbench:spec:result:0.1"},
        "execution_modes":["foreground"], "platforms":if change || schedule || fleet || runner || job || extension || operation == "config.check" || toil || operation == "report.create" || process || operation == "task.run" || grafana || context || diagnostics || evidence_store || runbook_write || operation == "runbook.status" { vec!["linux"] } else { vec!["linux","windows","macos"] },
        "dependencies":if operation == "config.check" { vec![json!({"name":"python3 (grafana-dashboard only)","kind":"interpreter","version_requirement":">=3.11"}),json!({"name":"promtool (prometheus-rules only)","kind":"binary","version_requirement":"exact reviewed 3.15.0 linux-amd64 SHA-256; explicit startup path"})] } else if operation == "task.run" { vec![json!({"name":"python3","kind":"interpreter","version_requirement":">=3.11"})] } else { vec![] }, "stability":"experimental", "default_timeout_ms":if grafana || diagnostics || collection {60000} else {30000},"max_timeout_ms":if grafana || diagnostics || collection {60000} else {300000},
        "required_permissions": if query || collection { vec!["grafana.query"] } else if operation == "process.exec" { vec!["command.execute"] } else { vec![] },
        "implementation":{"kind":"builtin","reference":format!("workbench-core:{operation}")}
    }))
}

pub fn availability(operation: &str) -> Value {
    if operation.starts_with("change.") {
        return json!({"status":if cfg!(target_os="linux"){"requires_configuration"}else{"unsupported_platform"},"reason":"Requires an explicit mTLS profile and runner-owned file grants with distinct requester/approver principals and an exact pinned validator. Hosted grants are unavailable."});
    }
    if operation.starts_with("schedule.") {
        return json!({"status":"requires_configuration","reason":"Requires an explicit private schedule store and frozen fleet authority. Work is a separate explicit foreground entry; hosted grants are unavailable."});
    }
    if operation.starts_with("fleet.") {
        return json!({"status":"requires_configuration","reason":"Requires an explicit private fleet store and, for new collection, a finite trusted inventory. Hosted grants are unavailable."});
    }
    if operation.starts_with("runner.") {
        return json!({"status":"requires_configuration","reason":"Requires an explicit mTLS controller profile and exact remote runner identity. Remote authority is independently checked by the runner."});
    }
    if operation.starts_with("job.") {
        return json!({"status":if cfg!(target_os="linux"){"requires_configuration"}else{"unsupported_platform"},"reason":"Requires an explicit private job store. Writes require recording consent; job.work requires the explicit operator worker entry, current operation authority and a separately admitted runtime. No automatic service or replay."});
    }
    if operation.starts_with("extension.") {
        return json!({"status":if !cfg!(target_os="linux") {"unsupported_platform"} else if operation == "extension.inspect" {"available"} else {"requires_configuration"},"reason":"Inert package inspection only; list/install require an explicit private extension store. Package installation alone grants no execution. Exact separate startup policy grants are required; hosted grants are unavailable."});
    }
    if operation.starts_with("toil.") {
        return json!({"status":if cfg!(target_os="linux"){"requires_configuration"}else{"unsupported_platform"},"reason":"Requires a separate private toil store and explicit opt-in consent for recording. Capture additionally requires installed task dependencies. Discovery opens no metadata; hosted grants are unavailable."});
    }
    if matches!(
        operation,
        "runbook.run" | "runbook.resume" | "runbook.status"
    ) {
        return json!({"status":if cfg!(target_os = "linux") {"requires_configuration"} else {"unsupported_platform"},"reason":"Requires an explicit private runbook journal; execution also requires the selected evidence store. Discovery reads neither store. Hosted grants are unavailable."});
    }
    if matches!(
        operation,
        "evidence.open" | "evidence.replay" | "report.create"
    ) {
        return json!({"status":if cfg!(all(target_os = "linux", target_arch = "x86_64")) {"requires_configuration"} else {"unsupported_platform"},"reason":"Requires an explicit private evidence store and Linux x86_64 seccomp offline profile. No source configuration, credentials or hosted grants are used."});
    }
    if operation.starts_with("evidence.") && !operation.starts_with("evidence.packs.") {
        return json!({"status":if cfg!(target_os = "linux") {"requires_configuration"} else {"unsupported_platform"},"reason":"Requires an explicit private evidence store. Collection additionally requires fresh context, a pinned Grafana connection and scoped query credentials. Discovery opens no store, source, provider or network connection; hosted grants are not implemented."});
    }
    if operation == "diagnostics.run" {
        return json!({"status":if cfg!(target_os = "linux") {"requires_configuration"} else {"unsupported_platform"},"reason":"Connectivity requires explicit trusted diagnostics configuration and allowlisted destinations. Discovery opens no source/CA file, resolver, socket or proc process metadata."});
    }
    if operation == "context.resolve" {
        return json!({"status":if cfg!(target_os = "linux") {"requires_configuration"} else {"unsupported_platform"},"reason":"Offline fixture-export resolution requires paired trusted source and review-age options. Discovery does not open the source or establish a live binding."});
    }
    if matches!(
        operation,
        "grafana.dashboard.get" | "grafana.query" | "query.run"
    ) {
        if !cfg!(target_os = "linux") {
            return json!({"status":"unsupported_platform","reason":"Native Grafana fixture acceptance is currently limited to Linux."});
        }
        return json!({"status":"requires_configuration","reason":"Native adapter installed; explicit operator config and a scoped environment credential are required. Discovery does not read secrets or contact a server."});
    }
    if operation == "config.check" {
        return json!({"status":if cfg!(target_os="linux") {"requires_configuration"} else {"unsupported_platform"},"reason":"Dependencies are kind-specific: dashboard-hygiene uses native Python >=3.11; prometheus-rules requires the reviewed promtool 3.15.0 linux-amd64 binary at an explicit startup path. Discovery reads no validator binary or model; missing Python does not block Prometheus rules."});
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
        "execution":availability("process.exec"), "tasks":crate::tasks::list()["tasks"], "grafana":availability("grafana.query"), "diagnostics":availability("diagnostics.run"), "record_modes":["never"], "evidence_collection_record_modes":["auto","always"], "evidence":availability("evidence.collect"), "default_timeout_ms":30000,"max_timeout_ms":300000,
        "max_capture_bytes_per_stream":1048576,"max_encoded_result_bytes":2097152,"max_request_bytes":65536,"max_request_depth":16,
        "child_environment":super::process::CHILD_ENV.into_iter().map(|(k,v)| (k.to_owned(), json!(v))).collect::<serde_json::Map<String, Value>>(),
        "stdin":"closed", "target":{"kind":"local","id":"workstation"},"profile":"operator-local",
        "limitations":["No executable version probe or network request is performed.","Process groups are not containment for deliberately escaping programs.","No protected installation or service health assessment is provided. Explicit evidence collection can retain sanitized receipts in a private configured store; other operations do not record. The optional linux-read-v1 profile requires an explicit trusted policy; this offline check does not establish its kernel availability."]})
}

#[cfg(test)]
mod schema_tests {
    use super::*;
    #[test]
    fn worker_discovery_preserves_unclassified_child_effects() {
        assert_eq!(capability("job.work").unwrap()["effects"], "unclassified");
        for operation in ["job.submit", "job.cancel"] {
            assert_eq!(capability(operation).unwrap()["effects"], "local_write");
        }
    }
    #[test]
    fn all_discovery_input_schema_references_resolve_to_installed_specifications() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/sre-workbench/schemas");
        for operation in OPERATIONS {
            let capability = capability(operation).unwrap();
            if let Some(reference) = capability["input_schema"]["$ref"].as_str() {
                let name = reference
                    .strip_prefix("urn:sre-workbench:spec:")
                    .and_then(|value| value.strip_suffix(":0.1"))
                    .expect("versioned input schema reference");
                assert!(
                    directory.join(format!("{name}.schema.json")).is_file(),
                    "{operation}: {reference}"
                );
            }
        }
        assert_eq!(
            capability("toil.prune").unwrap()["input_schema"],
            json!({"type":"object","additionalProperties":false})
        );
    }
}
