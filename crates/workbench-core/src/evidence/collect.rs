//! Fixed sequential collection. Only finished, credential-redacted query receipts reach storage.
use super::{
    pack,
    store::{ArtifactRef, CollectionLease, Store, StoreError},
};
use crate::{
    OperatorContext, RunControl, execute_collection_child, grafana, queries,
    request::{Problem, Request, bounded},
    result::{MAX_RECEIPT_DEPTH, OperationResult, Status, timestamp},
};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const MAX_METADATA: usize = 64 * 1024;
const FINALIZATION_ALLOWANCE: Duration = Duration::from_millis(2500);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pack {
    id: String,
    version: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    datasource: String,
    parameters: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    service: String,
    environment: String,
    #[serde(default, deserialize_with = "present_string")]
    team: Option<String>,
    #[serde(default, deserialize_with = "present_string")]
    deployment: Option<String>,
    pack: Pack,
    window: Value,
    metrics: Query,
    logs: Query,
    on_failure: String,
    #[serde(default, deserialize_with = "present_string")]
    expected_pack_sha256: Option<String>,
}

fn present_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

fn problem(code: &'static str) -> Problem {
    Problem::invalid(
        code,
        "The evidence collection could not satisfy its explicit input, binding or retention contract; private inputs are withheld.",
    )
}

struct Prepared {
    input: Input,
    pack: Value,
    window: Value,
    queries: [Value; 2],
}

fn prepare(inputs: &Value, started_at: &str) -> Result<Prepared, Problem> {
    if !inputs.is_object()
        || !inputs["pack"].is_object()
        || !inputs["window"].is_object()
        || ["metrics", "logs"]
            .iter()
            .any(|key| !inputs[key].is_object() || !inputs[key]["parameters"].is_object())
    {
        return Err(problem("invalid_evidence_input"));
    }
    let input: Input =
        serde_json::from_value(inputs.clone()).map_err(|_| problem("invalid_evidence_input"))?;
    if !bounded(&input.service, 1, 128)
        || !bounded(&input.environment, 1, 128)
        || input
            .team
            .as_ref()
            .is_some_and(|value| !bounded(value, 1, 128))
        || input
            .deployment
            .as_ref()
            .is_some_and(|value| !bounded(value, 1, 128))
        || !matches!(input.on_failure.as_str(), "continue" | "stop")
    {
        return Err(problem("invalid_evidence_input"));
    }
    let definition = pack::select(
        &input.pack.id,
        input.pack.version,
        input.expected_pack_sha256.as_deref(),
    )?;
    let window = queries::resolve_window(&input.window, started_at)?;
    let mut query_inputs = [Value::Null, Value::Null];
    for (index, choice) in [&input.metrics, &input.logs].into_iter().enumerate() {
        let identity = &definition["steps"][index]["query"];
        let query = json!({"id":identity["id"],"version":identity["version"],
            "expected_definition_sha256":identity["sha256"],"datasource":choice.datasource,
            "parameters":choice.parameters,"window":window});
        // Compile both children and all legacy guards before any filesystem reads or effects.
        // The private compiled job/expression is dropped here; only the admitted wire input
        // remains in memory until its actual child performs its own pinned admission.
        let compiled = grafana::prepare_collection_query(&query, started_at)?;
        if compiled.definition != *identity {
            return Err(problem("invalid_installed_evidence_pack"));
        }
        query_inputs[index] = query;
    }
    Ok(Prepared {
        input,
        pack: definition,
        window,
        queries: query_inputs,
    })
}

fn check(control: &RunControl, deadline: Instant) -> Result<(), Problem> {
    if control.signal.load(Ordering::Relaxed) != 0 {
        return Err(problem("cancelled"));
    }
    // Collection children admit whole milliseconds. Less than one remaining
    // millisecond cannot admit another child, even before the exact Instant.
    if work_budget_exhausted(deadline, Instant::now()) {
        return Err(problem("deadline_exceeded"));
    }
    #[cfg(target_os = "linux")]
    if let Some(fd) = control.result_fd {
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: one initialized pollfd is borrowed for a nonblocking observation.
        if unsafe { libc::poll(&mut poll, 1, 0) } > 0
            && poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
        {
            return Err(problem("result_consumer_closed"));
        }
    }
    Ok(())
}

fn work_budget_exhausted(deadline: Instant, now: Instant) -> bool {
    deadline.saturating_duration_since(now).as_millis() == 0
}

fn selectors(input: &Input) -> Value {
    let mut selectors = json!({"service":input.service,"environment":input.environment});
    if let Some(team) = &input.team {
        selectors["team"] = json!(team);
    }
    if let Some(deployment) = &input.deployment {
        selectors["deployment"] = json!(deployment);
    }
    selectors
}

fn context_projection(data: &Value, binding: &mut grafana::CollectionBinding) -> Value {
    let source = &data["source"];
    let record = &data["record"];
    json!({
        "source":{"kind":"fixture-export","id":binding.metadata(source["id"].clone()),
            "revision":binding.metadata(source["revision"].clone()),"sha256":binding.metadata(source["sha256"].clone()),
            "observed_at":binding.metadata(source["observed_at"].clone()),"evidence_state":"sourced"},
        "record":{"service":binding.metadata(record["service"].clone()),"team":binding.metadata(record["team"].clone()),
            "environment":binding.metadata(record["environment"].clone()),
            "deployment":record.get("deployment").map_or(Value::Null, |value| binding.metadata(value.clone())),
            "state":"active","last_reviewed":binding.metadata(record["last_reviewed"].clone()),
            "bindings":{"target":binding.metadata(record["bindings"]["target"].clone()),
                "environment":binding.metadata(record["bindings"]["environment"].clone()),
                "organization":binding.metadata(record["bindings"]["organization"].clone())}},
        "resolution":{"state":"resolved","usable":true,"max_age_days":data["resolution"]["max_age_days"],
            "age_seconds":data["resolution"]["age_seconds"]}
    })
}

fn initial_payload(
    prepared: &Prepared,
    context: &Value,
    binding: &mut grafana::CollectionBinding,
    result: &OperationResult,
) -> Value {
    let context = context_projection(context, binding);
    let selection = json!({"service":binding.metadata(json!(prepared.input.service)),
        "environment":binding.metadata(json!(prepared.input.environment)),
        "team":prepared.input.team.as_ref().map_or(Value::Null, |value| binding.metadata(json!(value))),
        "deployment":prepared.input.deployment.as_ref().map_or(Value::Null, |value| binding.metadata(json!(value)))});
    let window = json!({"from":binding.metadata(prepared.window["from"].clone()),"to":binding.metadata(prepared.window["to"].clone())});
    let steps = pack::STEP_IDS.iter().enumerate().map(|(index, id)| json!({
        "id":id,"operation":"query.run","definition":prepared.pack["steps"][index]["query"],
        "status":"omitted","run_id":null,"started_at":null,"finished_at":null,"duration_ms":null,
        "errors":[],"coverage":null,"redaction":null,"truncation":{"stdout":false,"stderr":false},
        "artifact":null,"omission":"not_dispatched"
    })).collect::<Vec<_>>();
    json!({"format_version":1,"producer":{"id":"save","version":env!("CARGO_PKG_VERSION"),"spec_version":"0.1"},
        "run_id":result.run_id,"request_id":result.request_id,"started_at":result.started_at,"finalized_at":null,
        "selection":selection,"connection":binding.connection,
        "provenance":{"grafana_config_sha256":binding.config_sha256,"grafana_ca":binding.ca,"context":context},
        "pack":{"id":prepared.pack["id"],"version":prepared.pack["version"],"sha256":prepared.pack["sha256"]},
        "window":window,"on_failure":prepared.input.on_failure,"capture":{"state":"none","expected_steps":2,"retained_receipts":0},
        "steps":steps,"errors":[],"redaction":binding.redaction(),"cancellation_signal":null})
}

fn observed_step(definition: Value, id: &str, child: &OperationResult) -> Value {
    json!({"id":id,"operation":"query.run","definition":definition,"status":child.execution.status,
        "run_id":child.run_id,"started_at":child.started_at,"finished_at":child.finished_at,"duration_ms":child.duration_ms,
        "errors":child.errors.iter().map(|error| error.code.as_str()).collect::<Vec<_>>(),
        "coverage":child.coverage,"redaction":child.data.get("redaction").cloned().unwrap_or(Value::Null),
        "truncation":{"stdout":child.output.stdout_truncated,"stderr":child.output.stderr_truncated},
        "artifact":null,"omission":"receipt_not_retained"})
}

fn set_omissions(payload: &mut Value, first: usize, reason: &str) {
    for step in payload["steps"]
        .as_array_mut()
        .expect("fixed steps")
        .iter_mut()
        .skip(first)
    {
        if step["status"] == "omitted" {
            step["omission"] = json!(reason);
        }
    }
}

fn retained(payload: &mut Value, artifacts: &BTreeMap<String, ArtifactRef>) {
    for step in payload["steps"].as_array_mut().expect("fixed steps") {
        if let Some(artifact) = step["id"].as_str().and_then(|id| artifacts.get(id)) {
            step["artifact"] = serde_json::to_value(artifact).expect("artifact reference");
            step["omission"] = Value::Null;
        }
    }
    payload["capture"]["retained_receipts"] = json!(artifacts.len());
    refresh_capture(payload);
}

fn refresh_capture(payload: &mut Value) {
    let count = payload["capture"]["retained_receipts"]
        .as_u64()
        .unwrap_or(0);
    let complete = count == 2
        && payload["errors"].as_array().is_some_and(Vec::is_empty)
        && payload["cancellation_signal"].is_null()
        && payload["steps"].as_array().is_some_and(|steps| {
            steps
                .iter()
                .all(|step| step["status"] == "succeeded" && !step["artifact"].is_null())
        });
    payload["capture"]["state"] = json!(if complete {
        "complete"
    } else if count > 0 {
        "partial"
    } else {
        "none"
    });
}

fn record_error(payload: &mut Value, result: &mut OperationResult, code: &str) {
    let errors = payload["errors"].as_array_mut().expect("collection errors");
    if !errors.iter().any(|error| error == code) {
        errors.push(json!(code));
        result.error(code, "The collection could not retain a complete successful set of observations; inspect the explicit step outcomes and retained references.");
    }
    refresh_capture(payload);
}

fn terminal_status(code: &str) -> Status {
    match code {
        "cancelled" | "result_consumer_closed" => Status::Cancelled,
        "deadline_exceeded" | "store_deadline_exceeded" => Status::TimedOut,
        _ => Status::Failed,
    }
}

fn reject_interruption(result: &mut OperationResult, control: &RunControl, problem: Problem) {
    result.execution.status = terminal_status(problem.code);
    result.error(problem.code, problem.message);
    let signal = control.signal.load(Ordering::Relaxed);
    if signal != 0 {
        result.data = json!({"cancellation_signal":signal});
    }
}

fn stop(
    payload: &mut Value,
    result: &mut OperationResult,
    control: &RunControl,
    code: &str,
    first: usize,
) {
    record_error(payload, result, code);
    set_omissions(payload, first, code);
    result.execution.status = terminal_status(code);
    let signal = control.signal.load(Ordering::Relaxed);
    if signal != 0 {
        payload["cancellation_signal"] = json!(signal);
    }
    refresh_capture(payload);
}

fn finish_deadline(deadline: Instant, established: &mut Option<Instant>, now: Instant) -> Instant {
    // Enter this phase only once. Rechecking an already-observed cancellation
    // cannot restart its allowance or extend any observation's original deadline.
    *established
        .get_or_insert((now + FINALIZATION_ALLOWANCE).min(deadline + FINALIZATION_ALLOWANCE))
}

fn storage_error(
    payload: &mut Value,
    result: &mut OperationResult,
    error: StoreError,
    first: usize,
) {
    record_error(payload, result, error.code);
    set_omissions(payload, first, "store_unavailable");
    if !matches!(
        result.execution.status,
        Status::Cancelled | Status::TimedOut
    ) {
        result.execution.status = terminal_status(error.code);
    }
    if error.effects || result.effect_outcome != "not_attempted" {
        result.effect_outcome = "unknown".into();
    }
}

fn publish_summary(mut payload: Value, result: &mut OperationResult, bundle: Value) {
    if payload["finalized_at"].is_null() {
        payload["finalized_at"] = json!(timestamp());
    }
    refresh_capture(&mut payload);
    result.artifacts = payload["steps"]
        .as_array()
        .expect("fixed steps")
        .iter()
        .filter(|step| !step["artifact"].is_null())
        .map(|step| step["artifact"].clone())
        .collect();
    result.coverage.state = "not_established".into();
    payload["bundle"] = bundle;
    payload["delivery_cancellation_signal"] = Value::Null;
    result.data = payload;
}

fn reservation(response_limit: usize) -> u64 {
    // Account for JSON/credential expansion, two retained receipts, one publication
    // temporary and bounded intent/checkpoint/manifest copies. Default budgets reserve
    // eight MiB; tighter response limits require proportionally less capacity.
    (response_limit as u64)
        .saturating_mul(36)
        .saturating_add(512 * 1024)
        .min(8 * 1024 * 1024)
}

pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    result.effective_limits.timeout_ms = request.limits.timeout_ms.min(60_000);
    let deadline = start + Duration::from_millis(result.effective_limits.timeout_ms);
    result.coverage.state = "not_established".into();
    result.coverage.scope = "Retained receipts for the fixed service queries; source-data coverage and service health remain unestablished.".into();
    result.coverage.limitations = vec![
        "Complete capture means successful receipt retention, not complete telemetry coverage or service health.".into(),
        "Known credential echoes are redacted; unrelated sensitive telemetry is not automatically detected.".into(),
        "Finalization may use up to 2500 ms for observed bytes and metadata; synchronous filesystem I/O cannot forcibly preempt an uninterruptible kernel operation.".into(),
    ];
    if request.record == "never" {
        result.reject(problem("recording_required"));
        return;
    }
    result.record_mode = if request.record == "always" {
        "always"
    } else {
        "auto"
    };
    let prepared = match prepare(&request.inputs, &result.started_at) {
        Ok(prepared) => prepared,
        Err(problem) => {
            result.reject(problem);
            return;
        }
    };
    let Some(options) = context.evidence_store.as_ref() else {
        result.reject(problem("evidence_store_required"));
        return;
    };
    if context.context_source_path.is_none() {
        result.reject(problem("context_source_required"));
        return;
    }
    if context
        .context_max_age_days
        .is_none_or(|days| !(1..=365).contains(&days))
    {
        result.reject(problem("context_age_policy_required"));
        return;
    }
    if let Err(problem) = check(control, deadline) {
        reject_interruption(result, control, problem);
        return;
    }
    let mut binding = match grafana::prepare_collection_binding(context, &request.target.id) {
        Ok(binding) => binding,
        Err(problem) => {
            result.reject(problem);
            return;
        }
    };
    if let Err(problem) = check(control, deadline) {
        reject_interruption(result, control, problem);
        return;
    }
    let mut context_request = Request::new("context.resolve", selectors(&prepared.input));
    context_request.limits = request.limits.clone();
    let resolved = execute_collection_child(context_request, control, context, None, deadline);
    if resolved.execution.status != Status::Succeeded
        || resolved.data["resolution"]["usable"] != true
    {
        result.execution.status = resolved.execution.status;
        if resolved.errors.is_empty() {
            result.error("context_unusable", problem("context_unusable").message);
        }
        for error in resolved.errors {
            result.error(&error.code, problem("context_unusable").message);
        }
        let signal = control.signal.load(Ordering::Relaxed);
        if signal != 0 {
            result.data = json!({"cancellation_signal":signal});
        }
        return;
    }
    let record = &resolved.data["record"];
    if record["state"] != "active"
        || record["environment"] != prepared.input.environment
        || record["bindings"]["target"] != request.target.id
        || record["bindings"]["environment"] != prepared.input.environment
        || record["bindings"]["organization"] != binding.organization
    {
        result.reject(problem("context_binding_mismatch"));
        return;
    }
    let mut payload = initial_payload(&prepared, &resolved.data, &mut binding, result);
    // The raw selected source record is no longer needed and must not enter persistence.
    drop(resolved);
    if let Err(problem) = validate_payload(&payload) {
        result.reject(problem);
        return;
    }
    result.resolved_target = json!({"connection":request.target.id,"profile":"operator-local","effects":"local_store_write"});
    if let Err(problem) = check(control, deadline) {
        stop(&mut payload, result, control, problem.code, 0);
        publish_summary(payload, result, Value::Null);
        return;
    }
    let mut store = match Store::open_validated(
        options,
        true,
        deadline,
        crate::evidence::validate_bundle,
        None,
    ) {
        Ok(store) => store,
        Err(error) => {
            storage_error(&mut payload, result, error, 0);
            publish_summary(payload, result, Value::Null);
            return;
        }
    };
    result.effect_outcome = "unknown".into();
    let mut lease = match store.begin_collection(
        &result.run_id,
        payload.clone(),
        reservation(request.limits.max_output_bytes),
    ) {
        Ok(lease) => lease,
        Err(error) => {
            storage_error(&mut payload, result, error, 0);
            publish_summary(payload, result, Value::Null);
            return;
        }
    };
    let mut failed = false;
    let mut finalization = None;
    for index in 0..pack::STEP_IDS.len() {
        if let Err(problem) = check(control, deadline) {
            stop(&mut payload, result, control, problem.code, index);
            lease.set_deadline(finish_deadline(deadline, &mut finalization, Instant::now()));
            break;
        }
        let mut child_request = Request::new("query.run", prepared.queries[index].clone());
        child_request.target = request.target.clone();
        child_request.limits = request.limits.clone();
        let child = execute_collection_child(
            child_request,
            control,
            context,
            Some(&binding.identity),
            deadline,
        );
        let definition = payload["steps"][index]["definition"].clone();
        payload["steps"][index] = observed_step(definition, pack::STEP_IDS[index], &child);
        let replacements = child.data["redaction"]["replacements"]
            .as_u64()
            .unwrap_or(0);
        payload["redaction"]["replacements"] = json!(
            payload["redaction"]["replacements"]
                .as_u64()
                .unwrap_or(0)
                .saturating_add(replacements)
        );
        payload["redaction"]["applied"] =
            json!(payload["redaction"]["replacements"].as_u64().unwrap_or(0) > 0);
        failed |= child.execution.status != Status::Succeeded;
        if child.execution.status != Status::Succeeded {
            record_error(&mut payload, result, "collection_step_failed");
        }
        let stop_after_failure = failed && prepared.input.on_failure == "stop";
        if stop_after_failure {
            set_omissions(&mut payload, index + 1, "after_failure");
        }
        let interruption = check(control, deadline).err();
        if let Some(problem) = &interruption {
            stop(&mut payload, result, control, problem.code, index + 1);
        }
        if index + 1 == pack::STEP_IDS.len() || stop_after_failure || interruption.is_some() {
            // The last observation is already finalized. Assign its bounded storage
            // allowance before serialization/publication, which may straddle the
            // original deadline. Later dispatch always checks `deadline`, never this
            // storage-only deadline. An earlier per-request timeout under `continue`
            // remains eligible for another observation while parent work time exists.
            lease.set_deadline(finish_deadline(deadline, &mut finalization, Instant::now()));
        }
        // json_bytes is the finalized child's exact canonical newline-terminated receipt.
        // No original parameter or compiled expression is appended after this point.
        if let Err(error) = lease.put_receipt(pack::STEP_IDS[index], &child.json_bytes()) {
            retained(&mut payload, lease.artifacts());
            storage_error(&mut payload, result, error, index + 1);
            publish_summary(payload, result, Value::Null);
            return;
        }
        retained(&mut payload, lease.artifacts());
        if let Err(error) = lease.checkpoint(payload.clone()) {
            storage_error(&mut payload, result, error, index + 1);
            publish_summary(payload, result, Value::Null);
            return;
        }
        if interruption.is_some() || stop_after_failure {
            break;
        }
    }
    if let Err(problem) = check(control, deadline) {
        stop(&mut payload, result, control, problem.code, 0);
        lease.set_deadline(finish_deadline(deadline, &mut finalization, Instant::now()));
    }
    if !matches!(
        result.execution.status,
        Status::Cancelled | Status::TimedOut
    ) {
        result.execution.status = if failed {
            Status::Partial
        } else {
            Status::Succeeded
        };
    }
    payload["finalized_at"] = json!(timestamp());
    refresh_capture(&mut payload);
    if let Err(problem) = validate_payload(&payload) {
        record_error(&mut payload, result, problem.code);
        result.execution.status = Status::Failed;
        publish_summary(payload, result, Value::Null);
        return;
    }
    let bundle = match seal(&mut lease, &payload) {
        Ok(bundle) => bundle,
        Err(error) => {
            storage_error(&mut payload, result, error, 2);
            publish_summary(payload, result, Value::Null);
            return;
        }
    };
    result.effect_outcome = "succeeded".into();
    result.sources.push(json!({"reference":"evidence:sealed-collection","observed_at":timestamp(),"evidence_state":"sourced","untrusted":true}));
    publish_summary(payload, result, bundle);
}

fn seal(lease: &mut CollectionLease<'_>, payload: &Value) -> Result<Value, StoreError> {
    lease
        .seal(payload.clone())
        .map(|bundle| serde_json::to_value(bundle).expect("bundle reference"))
}

fn keys(value: &Value, names: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == names.len() && names.iter().all(|key| object.contains_key(*key))
    })
}

fn marker(value: &Value) -> bool {
    keys(value, &["redacted"]) && value["redacted"] == true
}
fn text(value: &Value, maximum: usize) -> bool {
    value
        .as_str()
        .is_some_and(|value| bounded(value, 1, maximum))
}
fn private_text(value: &Value, maximum: usize) -> bool {
    marker(value) || text(value, maximum)
}
fn optional_private_text(value: &Value, maximum: usize) -> bool {
    value.is_null() || private_text(value, maximum)
}
fn private_equal(left: &Value, right: &Value) -> bool {
    marker(left) || marker(right) || left == right
}
fn date(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|value| value.len() <= 64 && OffsetDateTime::parse(value, &Rfc3339).is_ok())
}
fn private_date(value: &Value) -> bool {
    marker(value) || date(value)
}
fn private_digest(value: &Value) -> bool {
    marker(value)
        || value
            .as_str()
            .and_then(|value| value.strip_prefix("sha256:"))
            .is_some_and(pack::digest)
}
fn errors(value: &Value) -> bool {
    value.as_array().is_some_and(|errors| {
        errors.len() <= 16
            && errors
                .iter()
                .all(|error| error.as_str().is_some_and(crate::request::valid_operation))
    })
}
fn redaction(value: &Value) -> bool {
    keys(value, &["applied", "replacements"])
        && value["replacements"]
            .as_u64()
            .is_some_and(|count| value["applied"].as_bool() == Some(count > 0))
}

/// Readers validate the same strict collector payload before showing or comparing it.
/// The store envelope's interrupted state must still override any earlier payload state.
pub(crate) fn validate_payload(payload: &Value) -> Result<(), Problem> {
    let invalid = || problem("invalid_evidence_payload");
    if serde_json::to_vec(payload).map_err(|_| invalid())?.len() > MAX_METADATA
        || !keys(
            payload,
            &[
                "format_version",
                "producer",
                "run_id",
                "request_id",
                "started_at",
                "finalized_at",
                "selection",
                "connection",
                "provenance",
                "pack",
                "window",
                "on_failure",
                "capture",
                "steps",
                "errors",
                "redaction",
                "cancellation_signal",
            ],
        )
        || payload["format_version"] != 1
        || !keys(&payload["producer"], &["id", "version", "spec_version"])
        || payload["producer"]["id"] != "save"
        || payload["producer"]["spec_version"] != "0.1"
        || !text(&payload["producer"]["version"], 64)
        || !text(&payload["run_id"], 128)
        || !payload["run_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("run-") && crate::request::valid_operation(id))
        || !text(&payload["request_id"], 128)
        || !date(&payload["started_at"])
        || !(payload["finalized_at"].is_null() || date(&payload["finalized_at"]))
        || !matches!(payload["on_failure"].as_str(), Some("stop" | "continue"))
        || !errors(&payload["errors"])
        || !redaction(&payload["redaction"])
        || !(payload["cancellation_signal"].is_null()
            || payload["cancellation_signal"]
                .as_u64()
                .is_some_and(|signal| signal > 0))
    {
        return Err(invalid());
    }
    let selection = &payload["selection"];
    let connection = &payload["connection"];
    if !keys(selection, &["service", "environment", "team", "deployment"])
        || !["service", "environment"]
            .iter()
            .all(|key| private_text(&selection[key], 128))
        || !["team", "deployment"]
            .iter()
            .all(|key| optional_private_text(&selection[key], 128))
        || !keys(connection, &["id", "origin", "organization_id"])
        || !connection["id"]
            .as_str()
            .is_some_and(crate::request::valid_operation)
        || !private_text(&connection["origin"], 2048)
        || !(marker(&connection["organization_id"])
            || connection["organization_id"]
                .as_i64()
                .is_some_and(|id| id > 0))
    {
        return Err(invalid());
    }
    let provenance = &payload["provenance"];
    let ca = &provenance["grafana_ca"];
    let context = &provenance["context"];
    let source = &context["source"];
    let record = &context["record"];
    let resolution = &context["resolution"];
    if !keys(
        provenance,
        &["grafana_config_sha256", "grafana_ca", "context"],
    ) || !private_digest(&provenance["grafana_config_sha256"])
        || !keys(ca, &["kind", "sha256"])
        || !((ca["kind"] == "custom" && private_digest(&ca["sha256"]))
            || (ca["kind"] == "builtin" && ca["sha256"].is_null()))
        || !keys(context, &["source", "record", "resolution"])
        || !keys(
            source,
            &[
                "kind",
                "id",
                "revision",
                "sha256",
                "observed_at",
                "evidence_state",
            ],
        )
        || source["kind"] != "fixture-export"
        || source["evidence_state"] != "sourced"
        || !private_text(&source["id"], 128)
        || !private_text(&source["revision"], 128)
        || !private_digest(&source["sha256"])
        || !private_date(&source["observed_at"])
        || !keys(
            record,
            &[
                "service",
                "team",
                "environment",
                "deployment",
                "state",
                "last_reviewed",
                "bindings",
            ],
        )
        || !["service", "team", "environment"]
            .iter()
            .all(|key| private_text(&record[key], 128))
        || !optional_private_text(&record["deployment"], 128)
        || record["state"] != "active"
        || !private_date(&record["last_reviewed"])
        || !keys(
            &record["bindings"],
            &["target", "environment", "organization"],
        )
        || !["target", "environment", "organization"]
            .iter()
            .all(|key| private_text(&record["bindings"][key], 128))
        || !keys(
            resolution,
            &["state", "usable", "max_age_days", "age_seconds"],
        )
        || resolution["state"] != "resolved"
        || resolution["usable"] != true
        || !resolution["max_age_days"]
            .as_u64()
            .is_some_and(|days| (1..=365).contains(&days))
        || resolution["age_seconds"].as_u64().is_none()
    {
        return Err(invalid());
    }
    if !private_equal(&selection["service"], &record["service"])
        || !private_equal(&selection["environment"], &record["environment"])
        || !private_equal(
            &selection["environment"],
            &record["bindings"]["environment"],
        )
        || !private_equal(&connection["id"], &record["bindings"]["target"])
        || ["team", "deployment"]
            .iter()
            .any(|key| !selection[key].is_null() && !private_equal(&selection[key], &record[key]))
        || resolution["age_seconds"].as_u64().unwrap_or(u64::MAX)
            > resolution["max_age_days"].as_u64().unwrap_or(0) * 86_400
    {
        return Err(invalid());
    }
    if !marker(&record["bindings"]["organization"])
        && !marker(&connection["organization_id"])
        && record["bindings"]["organization"].as_str()
            != connection["organization_id"]
                .as_i64()
                .map(|organization| organization.to_string())
                .as_deref()
    {
        return Err(invalid());
    }
    let installed = pack::installed()?;
    if !keys(&payload["pack"], &["id", "version", "sha256"])
        || payload["pack"]
            != json!({"id":installed["id"],"version":installed["version"],"sha256":installed["sha256"]})
        || !keys(&payload["window"], &["from", "to"])
        || !["from", "to"].iter().all(|key| {
            marker(&payload["window"][key])
                || payload["window"][key]
                    .as_str()
                    .and_then(grafana::date)
                    .is_some()
        })
    {
        return Err(invalid());
    }
    if let (Some(from), Some(to)) = (
        payload["window"]["from"].as_str().and_then(grafana::date),
        payload["window"]["to"].as_str().and_then(grafana::date),
    ) && (from.1 >= to.1 || to.1 - from.1 > 86_400_000)
    {
        return Err(invalid());
    }
    let steps = payload["steps"]
        .as_array()
        .filter(|steps| steps.len() == 2)
        .ok_or_else(invalid)?;
    let mut count = 0;
    for (index, step) in steps.iter().enumerate() {
        if !keys(
            step,
            &[
                "id",
                "operation",
                "definition",
                "status",
                "run_id",
                "started_at",
                "finished_at",
                "duration_ms",
                "errors",
                "coverage",
                "redaction",
                "truncation",
                "artifact",
                "omission",
            ],
        ) || step["id"] != pack::STEP_IDS[index]
            || step["operation"] != "query.run"
            || step["definition"] != installed["steps"][index]["query"]
            || !errors(&step["errors"])
            || !(step["redaction"].is_null() || redaction(&step["redaction"]))
            || !keys(&step["truncation"], &["stdout", "stderr"])
            || !step["truncation"]["stdout"].is_boolean()
            || !step["truncation"]["stderr"].is_boolean()
            || !(step["omission"].is_null()
                || matches!(
                    step["omission"].as_str(),
                    Some(
                        "not_dispatched"
                            | "after_failure"
                            | "cancelled"
                            | "deadline_exceeded"
                            | "result_consumer_closed"
                            | "store_unavailable"
                            | "receipt_not_retained"
                    )
                ))
        {
            return Err(invalid());
        }
        if step["status"] == "omitted" {
            if ![
                "run_id",
                "started_at",
                "finished_at",
                "duration_ms",
                "artifact",
                "coverage",
                "redaction",
            ]
            .iter()
            .all(|key| step[key].is_null())
                || step["omission"].is_null()
                || !step["errors"].as_array().is_some_and(Vec::is_empty)
                || step["truncation"]["stdout"] != false
                || step["truncation"]["stderr"] != false
            {
                return Err(invalid());
            }
        } else if !matches!(
            step["status"].as_str(),
            Some(
                "succeeded"
                    | "failed"
                    | "partial"
                    | "denied"
                    | "unsupported"
                    | "cancelled"
                    | "timed_out"
                    | "unknown"
            )
        ) || !text(&step["run_id"], 128)
            || !date(&step["started_at"])
            || !date(&step["finished_at"])
            || step["duration_ms"].as_u64().is_none()
            || (!step["artifact"].is_null() && !step["omission"].is_null())
            || (step["artifact"].is_null() && step["omission"] != "receipt_not_retained")
        {
            return Err(invalid());
        }
        if !step["coverage"].is_null() {
            let coverage = &step["coverage"];
            if !keys(coverage, &["state", "scope", "limitations"])
                || !text(&coverage["state"], 64)
                || !text(&coverage["scope"], 2048)
                || !coverage["limitations"].as_array().is_some_and(|values| {
                    values.len() <= 16 && values.iter().all(|value| text(value, 2048))
                })
            {
                return Err(invalid());
            }
        }
        if step["status"] != "omitted" && step["coverage"].is_null() {
            return Err(invalid());
        }
        if step["status"] == "succeeded"
            && (!step["errors"].as_array().is_some_and(Vec::is_empty)
                || step["redaction"].is_null()
                || step["truncation"]["stdout"] != false
                || step["truncation"]["stderr"] != false)
        {
            return Err(invalid());
        }
        if !step["artifact"].is_null() {
            let artifact = serde_json::from_value::<ArtifactRef>(step["artifact"].clone())
                .map_err(|_| invalid())?;
            if artifact.media_type != "application/json"
                || artifact.classification != "restricted"
                || !(1..=crate::request::MAX_RESULT_BYTES as u64).contains(&artifact.bytes)
                || !artifact
                    .digest
                    .strip_prefix("sha256:")
                    .is_some_and(pack::digest)
                || artifact.path
                    != format!(
                        "artifacts/{}.json",
                        artifact.digest.strip_prefix("sha256:").unwrap_or("")
                    )
            {
                return Err(invalid());
            }
            count += 1;
        }
    }
    let capture = &payload["capture"];
    let complete = count == 2
        && steps.iter().all(|step| step["status"] == "succeeded")
        && payload["errors"].as_array().is_some_and(Vec::is_empty)
        && payload["cancellation_signal"].is_null();
    let expected_state = if complete {
        "complete"
    } else if count > 0 {
        "partial"
    } else {
        "none"
    };
    if !keys(capture, &["state", "expected_steps", "retained_receipts"])
        || capture["expected_steps"] != 2
        || capture["retained_receipts"] != count
        || capture["state"] != expected_state
    {
        return Err(invalid());
    }
    Ok(())
}

/// Reconcile each immutable receipt with its compact manifest summary. A crash may
/// publish an artifact before its newer summary, but only an interrupted envelope can
/// use that exception; its receipt still has to belong to the fixed query and target.
pub(crate) fn validate_receipt(
    payload: &Value,
    step_id: &str,
    bytes: &[u8],
    interrupted: bool,
) -> Result<(), Problem> {
    let invalid = || problem("invalid_evidence_receipt");
    let index = pack::STEP_IDS
        .iter()
        .position(|id| *id == step_id)
        .ok_or_else(invalid)?;
    let receipt =
        crate::request::parse_bounded_json(bytes, MAX_RECEIPT_DEPTH).map_err(|_| invalid())?;
    let step = &payload["steps"][index];
    if receipt["spec_version"] != "0.1"
        || receipt["operation"] != "query.run"
        || receipt["operation_version"] != 1
        || receipt["record_mode"] != "never"
        || receipt["target"]["kind"] != "connection"
        || receipt["target"]["id"] != payload["connection"]["id"]
        || receipt["target"].get("environment").is_some()
        || !matches!(
            receipt["effect_outcome"].as_str(),
            Some("not_attempted" | "not_applicable")
        )
        || receipt["assessment"] != "not_assessed"
        || !text(&receipt["run_id"], 128)
        || !date(&receipt["started_at"])
        || !date(&receipt["finished_at"])
        || receipt["duration_ms"].as_u64().is_none()
    {
        return Err(invalid());
    }
    let data = &receipt["data"];
    let succeeded = receipt["execution"]["status"] == "succeeded";
    if (!data["definition"].is_null() && data["definition"] != step["definition"])
        || (succeeded && data["definition"] != step["definition"])
        || (!data["window"].is_null() && data["window"] != payload["window"])
        || (succeeded && data["window"] != payload["window"])
    {
        return Err(invalid());
    }
    if !data["binding"].is_null()
        && (data["binding"]["origin"] != payload["connection"]["origin"]
            || data["binding"]["organization_id"] != payload["connection"]["organization_id"])
    {
        return Err(invalid());
    }
    if interrupted && step["artifact"].is_null() {
        return Ok(());
    }
    let errors = receipt["errors"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|error| error.get("code").cloned().ok_or_else(invalid))
        .collect::<Result<Vec<_>, _>>()?;
    if step["status"] != receipt["execution"]["status"]
        || [
            "run_id",
            "started_at",
            "finished_at",
            "duration_ms",
            "coverage",
        ]
        .iter()
        .any(|key| step[key] != receipt[key])
        || step["errors"] != json!(errors)
        || step["redaction"] != data.get("redaction").cloned().unwrap_or(Value::Null)
        || step["truncation"]["stdout"] != receipt["output"]["stdout_truncated"]
        || step["truncation"]["stderr"] != receipt["output"]["stderr_truncated"]
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANCHOR: &str = "2026-10-10T12:34:56.123987654Z";

    #[test]
    fn rounded_child_deadline_exhausts_collection_work_before_parent_instant() {
        let start = Instant::now();
        let parent = start + Duration::from_micros(900_750);
        // A child entered 500 us later receives the same whole-ms budget as
        // execute_collection_child: its deadline precedes the parent by 250 us.
        let child_start = start + Duration::from_micros(500);
        let remaining_ms = parent.duration_since(child_start).as_millis() as u64;
        let child_deadline = child_start + Duration::from_millis(remaining_ms);
        assert!(child_deadline < parent);
        assert!(work_budget_exhausted(parent, child_deadline));
        assert!(work_budget_exhausted(parent, parent));
        // A shorter per-request timeout does not exhaust an otherwise available
        // parent budget, so continue may still dispatch its independent query.
        assert!(!work_budget_exhausted(
            parent,
            start + Duration::from_millis(20)
        ));
        assert!(!work_budget_exhausted(
            parent,
            parent - Duration::from_millis(1)
        ));
    }

    #[test]
    fn finalization_allowance_is_fixed_at_first_transition_and_capped_by_parent() {
        let now = Instant::now();
        let parent = now + Duration::from_secs(60);
        let mut established = None;
        let final_deadline = finish_deadline(parent, &mut established, now);
        assert_eq!(
            final_deadline.duration_since(now),
            Duration::from_millis(2500)
        );
        // A later signal/deadline check cannot grant another cleanup period.
        assert_eq!(
            finish_deadline(parent, &mut established, now + Duration::from_millis(2400)),
            final_deadline
        );
        let mut late = None;
        assert_eq!(
            finish_deadline(parent, &mut late, parent + Duration::from_millis(500)),
            parent + Duration::from_millis(2500)
        );
    }

    fn input() -> Value {
        json!({"service":"fixture","environment":"dev","pack":{"id":"service-queries","version":1},
        "window":{"kind":"relative","lookback_seconds":300},"metrics":{"datasource":"metrics-1","parameters":{"job":"fixture"}},
        "logs":{"datasource":"logs-1","parameters":{"job":"fixture"}},"on_failure":"continue"})
    }

    #[test]
    fn collection_compiles_every_parameter_before_any_binding() {
        let prepared = prepare(&input(), ANCHOR).unwrap();
        assert_eq!(prepared.queries[0]["window"], prepared.queries[1]["window"]);
        assert_eq!(
            prepared.window,
            json!({"kind":"absolute","from":"2026-10-10T12:29:56.123Z","to":"2026-10-10T12:34:56.123Z"})
        );
        for key in ["metrics", "logs"] {
            let mut value = input();
            value[key]["parameters"]["job"] = json!("$__interval");
            assert_eq!(
                prepare(&value, ANCHOR).err().unwrap().code,
                "unresolved_expression"
            );
            value = input();
            value[key]["datasource"] = json!("../private");
            assert_eq!(
                prepare(&value, ANCHOR).err().unwrap().code,
                "invalid_grafana_input"
            );
        }
    }

    #[test]
    fn collection_rejects_null_unknown_steps_and_digest_without_source_reads() {
        for key in [
            "service",
            "environment",
            "team",
            "deployment",
            "pack",
            "window",
            "metrics",
            "logs",
            "on_failure",
            "expected_pack_sha256",
        ] {
            let mut value = input();
            value[key] = Value::Null;
            assert!(prepare(&value, ANCHOR).is_err(), "{key}");
        }
        for key in ["steps", "receipts", "operation", "state_dir", "parameters"] {
            let mut value = input();
            value[key] = json!([]);
            assert!(prepare(&value, ANCHOR).is_err(), "{key}");
        }
        let mut value = input();
        value["expected_pack_sha256"] = json!("0".repeat(64));
        assert_eq!(
            prepare(&value, ANCHOR).err().unwrap().code,
            "pack_digest_mismatch"
        );
        value = input();
        value["pack"]["version"] = json!(2);
        assert_eq!(
            prepare(&value, ANCHOR).err().unwrap().code,
            "unknown_evidence_pack"
        );
    }

    #[test]
    fn receipt_summary_must_keep_the_original_finalized_observation() {
        let prepared = prepare(&input(), ANCHOR).unwrap();
        let mut request = Request::new("query.run", prepared.queries[0].clone());
        request.target = crate::request::Target {
            kind: "connection".into(),
            id: "fixture".into(),
            environment: None,
        };
        let mut child = OperationResult::new(&request);
        child.effect_outcome = "not_applicable".into();
        child.data = json!({"definition":prepared.pack["steps"][0]["query"],
            "window":{"from":prepared.window["from"],"to":prepared.window["to"]},
            "binding":{"origin":"https://example.invalid","organization_id":7},"redaction":{"applied":false,"replacements":0}});
        child.finish(Instant::now());
        let summary = observed_step(
            prepared.pack["steps"][0]["query"].clone(),
            "metrics",
            &child,
        );
        let mut payload = json!({"connection":{"id":"fixture","origin":"https://example.invalid","organization_id":7},
            "window":child.data["window"],"steps":[summary]});
        let bytes = child.json_bytes();
        assert!(validate_receipt(&payload, "metrics", &bytes, false).is_ok());
        payload["steps"][0]["duration_ms"] = json!(999_999);
        assert!(validate_receipt(&payload, "metrics", &bytes, false).is_err());
        assert!(validate_receipt(&payload, "metrics", &bytes, true).is_ok());
        payload["connection"]["id"] = json!("different");
        assert!(validate_receipt(&payload, "metrics", &bytes, true).is_err());
    }
}
