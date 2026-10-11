//! Reviewed, offline observation workflow with explicit durable human checkpoints.
mod definition;
mod store;
use crate::{
    OperatorContext, RunControl,
    request::{Problem, Request, Target},
    result::{OperationResult, Status, timestamp},
};
use definition::{hash, identity};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Reference {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_manifest_sha256: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Input {
    before: Reference,
    after: Reference,
    normalization: String,
    on_failure: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Fresh {
    id: String,
    expected_revision: u64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StepState {
    id: String,
    state: String,
    receipt: Option<Value>,
    condition: Option<bool>,
    acknowledged_at: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    format_version: u64,
    run_id: String,
    revision: u64,
    definition: Value,
    target: Target,
    input: Input,
    input_sha256: String,
    pinned: Input,
    created_at: String,
    updated_at: String,
    state: String,
    in_flight: Option<String>,
    next: usize,
    steps: Vec<StepState>,
    remaining_timeout_ms: u64,
    fresh_from: Option<Fresh>,
}
fn invalid() -> &'static str {
    "runbook_invalid_input"
}
fn exact(v: &Value, required: &[&str], optional: &[&str]) -> Result<(), &'static str> {
    let o = v.as_object().ok_or_else(invalid)?;
    if required.iter().any(|k| !o.contains_key(*k))
        || o.keys()
            .any(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
        || o.values().any(Value::is_null)
    {
        return Err(invalid());
    }
    Ok(())
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn reference(r: &Reference) -> bool {
    crate::evidence::store::valid_run_id(&r.id)
        && r.expected_manifest_sha256
            .as_ref()
            .is_none_or(|s| s.strip_prefix("sha256:").is_some_and(digest))
}
fn input(v: &Value) -> Result<Input, &'static str> {
    exact(v, &["before", "after", "normalization", "on_failure"], &[])?;
    for key in ["before", "after"] {
        exact(&v[key], &["id"], &["expected_manifest_sha256"])?;
    }
    let i: Input = serde_json::from_value(v.clone()).map_err(|_| invalid())?;
    if !reference(&i.before)
        || !reference(&i.after)
        || !matches!(
            i.normalization.as_str(),
            "exact-window-v1" | "window-offset-v1"
        )
        || !matches!(i.on_failure.as_str(), "stop" | "continue")
    {
        return Err(invalid());
    }
    Ok(i)
}
fn input_hash(i: &Input) -> String {
    hash(&serde_json::to_vec(i).expect("typed input"))
}
fn reject(result: &mut OperationResult, code: &'static str) {
    result.reject(Problem::invalid(code,"The guided runbook request or durable state did not satisfy its pinned contract; no step was substituted or replayed."));
}
fn active(control: &RunControl, deadline: Instant) -> Result<(), &'static str> {
    if control.signal.load(Ordering::Relaxed) != 0 {
        return Err("cancelled");
    }
    if Instant::now() >= deadline {
        return Err("deadline_exceeded");
    }
    #[cfg(target_os = "linux")]
    if let Some(fd) = control.result_fd {
        let mut p = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        }; // SAFETY: initialized single descriptor, zero-timeout poll.
        if unsafe { libc::poll(&mut p, 1, 0) } > 0
            && p.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
        {
            return Err("result_consumer_closed");
        }
    }
    Ok(())
}
fn revalidation_error(code: &'static str) -> &'static str {
    if matches!(
        code,
        "cancelled"
            | "deadline_exceeded"
            | "result_consumer_closed"
            | "store_cancelled"
            | "store_deadline_exceeded"
            | "store_busy"
    ) {
        code
    } else {
        "runbook_source_changed"
    }
}
fn pin(
    i: &Input,
    context: &OperatorContext,
    control: &RunControl,
    deadline: Instant,
) -> Result<Input, &'static str> {
    let options = context
        .evidence_store
        .as_ref()
        .ok_or("evidence_store_required")?;
    let store = crate::evidence::store::Store::open_validated(
        options,
        false,
        deadline,
        crate::evidence::validate_bundle,
        Some(Arc::clone(&control.signal)),
    )
    .map_err(|e| e.code)?;
    let mut p = i.clone();
    for r in [&mut p.before, &mut p.after] {
        active(control, deadline)?;
        let b = store.load_verified(&r.id).map_err(|e| e.code)?;
        if r.expected_manifest_sha256
            .as_ref()
            .is_some_and(|v| *v != b.reference.digest)
        {
            return Err("runbook_source_changed");
        }
        r.expected_manifest_sha256 = Some(b.reference.digest);
    }
    Ok(p)
}
fn valid_timestamp(s: &str) -> bool {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).is_ok()
}
fn validate(s: &State) -> Result<(), &'static str> {
    let bad = || "runbook_corrupt_state";
    if s.definition != identity() {
        return Err("runbook_definition_mismatch");
    }
    if s.format_version != 1
        || !crate::evidence::store::valid_run_id(&s.run_id)
        || s.revision == 0
        || s.remaining_timeout_ms > 300_000
        || !valid_timestamp(&s.created_at)
        || !valid_timestamp(&s.updated_at)
        || s.input
            .before
            .expected_manifest_sha256
            .as_ref()
            .is_some_and(|p| Some(p) != s.pinned.before.expected_manifest_sha256.as_ref())
        || s.input
            .after
            .expected_manifest_sha256
            .as_ref()
            .is_some_and(|p| Some(p) != s.pinned.after.expected_manifest_sha256.as_ref())
        || s.fresh_from.as_ref().is_some_and(|f| {
            !crate::evidence::store::valid_run_id(&f.id)
                || f.expected_revision == 0
                || f.id == s.run_id
        })
        || s.definition != identity()
        || s.target.kind != "local"
        || s.target.id != "workstation"
        || s.input_sha256 != input_hash(&s.input)
        || input(&serde_json::to_value(&s.input).unwrap()).is_err()
        || input(&serde_json::to_value(&s.pinned).unwrap()).is_err()
        || s.input.normalization != s.pinned.normalization
        || s.input.on_failure != s.pinned.on_failure
        || s.input.before.id != s.pinned.before.id
        || s.input.after.id != s.pinned.after.id
        || s.pinned.before.expected_manifest_sha256.is_none()
        || s.pinned.after.expected_manifest_sha256.is_none()
        || s.steps.len() != 6
        || s.next > 6
        || !matches!(
            s.state.as_str(),
            "ready" | "running" | "waiting" | "completed" | "failed" | "interrupted"
        )
    {
        return Err(bad());
    }
    let d = definition::installed()?;
    for (idx, step) in s.steps.iter().enumerate() {
        if step.id != d.steps[idx].id
            || !matches!(
                step.state.as_str(),
                "pending" | "completed" | "failed" | "skipped" | "waiting" | "acknowledged"
            )
        {
            return Err(bad());
        }
        if s.state == "failed" && idx >= s.next {
            if step.state != "skipped"
                || step.receipt.is_some()
                || step.condition.is_some()
                || step.acknowledged_at.is_some()
            {
                return Err(bad());
            }
            continue;
        }
        if idx < 3 {
            if step.condition.is_some() || step.acknowledged_at.is_some() {
                return Err(bad());
            }
            if matches!(step.state.as_str(), "completed" | "failed") != step.receipt.is_some() {
                return Err(bad());
            }
            if let Some(r) = &step.receipt
                && (r["operation"] != d.steps[idx].capability
                    || r["operation_version"] != 1
                    || r["spec_version"] != "0.1"
                    || r["target"] != serde_json::to_value(&s.target).unwrap()
                    || !r["execution"]["status"].is_string()
                    || !r["started_at"].as_str().is_some_and(valid_timestamp)
                    || !r["finished_at"].as_str().is_some_and(valid_timestamp)
                    || !r["run_id"]
                        .as_str()
                        .is_some_and(crate::evidence::store::valid_run_id)
                    || r["record_mode"] != "never")
            {
                return Err(bad());
            }
        } else if step.receipt.is_some()
            || step.acknowledged_at.is_some() != (step.state == "acknowledged")
        {
            return Err(bad());
        }
        if idx < 3 {
            if matches!(step.state.as_str(), "waiting" | "acknowledged")
                || (step.state == "skipped" && idx != 2)
            {
                return Err(bad());
            }
            if let Some(r) = &step.receipt {
                if !r["coverage"]["state"]
                    .as_str()
                    .is_some_and(|s| s.len() <= 64)
                    || !r["coverage"]["scope"]
                        .as_str()
                        .is_some_and(|s| s.len() <= 4096)
                    || !r["coverage"]["limitations"].as_array().is_some_and(|a| {
                        a.len() <= 64
                            && a.iter()
                                .all(|v| v.as_str().is_some_and(|s| s.len() <= 1024))
                    })
                    || !r["errors"].as_array().is_some_and(|a| {
                        a.len() <= 32
                            && a.iter().all(|v| {
                                v["code"].as_str().is_some_and(|s| s.len() <= 128)
                                    && v["message"].as_str().is_some_and(|s| s.len() <= 4096)
                                    && v["retryable"].is_boolean()
                            })
                    })
                {
                    return Err(bad());
                }
                if idx == 2 && serde_json::to_vec(&r["data"]).map_err(|_| bad())?.len() > 512 * 1024
                {
                    return Err(bad());
                }
                let status = r["execution"]["status"].as_str().ok_or_else(bad)?;
                if !matches!(
                    status,
                    "succeeded"
                        | "partial"
                        | "failed"
                        | "denied"
                        | "unsupported"
                        | "cancelled"
                        | "timed_out"
                        | "unknown"
                ) {
                    return Err(bad());
                }
                let usable = if idx == 2 {
                    matches!(status, "succeeded" | "partial")
                        && matches!(
                            r["data"]["state"].as_str(),
                            Some("complete" | "partial" | "incompatible")
                        )
                } else {
                    status == "succeeded"
                };
                if (step.state == "completed") != usable {
                    return Err(bad());
                }
            }
        } else {
            let matched = observation(s) == ["complete", "partial", "incompatible"][idx - 3];
            if matches!(step.state.as_str(), "completed" | "failed")
                || step
                    .acknowledged_at
                    .as_ref()
                    .is_some_and(|t| !valid_timestamp(t))
                || (step.state == "pending" && step.condition.is_some())
                || (step.state != "pending" && step.condition != Some(matched))
                || (matches!(step.state.as_str(), "waiting" | "acknowledged") && !matched)
                || (step.state == "skipped" && matched)
            {
                return Err(bad());
            }
        }
        if idx < s.next && matches!(step.state.as_str(), "pending" | "waiting") {
            return Err(bad());
        }
        if idx > s.next && step.state != "pending" {
            return Err(bad());
        }
    }
    if s.state == "failed" && !s.steps[..3].iter().any(|step| step.state == "failed") {
        return Err(bad());
    }
    if s.state == "ready" && (s.next != 0 || s.in_flight.is_some())
        || s.next > 3 && s.steps[2].state != "completed"
        || s.state == "completed"
            && s.steps
                .iter()
                .filter(|step| step.state == "acknowledged")
                .count()
                != 1
        || s.state == "completed" && s.next != 6
        || s.state == "waiting"
            && (s.next < 3
                || s.next >= 6
                || s.steps[s.next].state != "waiting"
                || s.in_flight.is_some())
        || s.in_flight
            .as_ref()
            .is_some_and(|id| s.next >= 3 || *id != s.steps[s.next].id || s.state != "running")
    {
        return Err(bad());
    }
    Ok(())
}
pub(crate) fn list() -> Value {
    json!({"runbooks":[identity()]})
}
pub(crate) fn show(inputs: &Value, result: &mut OperationResult) {
    if exact(inputs, &["id", "version"], &[]).is_err() {
        reject(result, invalid());
        return;
    }
    if inputs["id"] != "evidence-review" || inputs["version"] != 1 {
        reject(result, "runbook_unknown_definition");
        return;
    }
    match definition::installed() {
        Ok(d) => {
            result.data = serde_json::to_value(d).unwrap();
            result.data["sha256"] = json!(definition::SHA);
            result.execution.status = Status::Succeeded;
            result.effect_outcome = "not_applicable".into();
        }
        Err(e) => reject(result, e),
    }
}
fn observation(s: &State) -> Value {
    s.steps[2]
        .receipt
        .as_ref()
        .map(|r| r["data"]["state"].clone())
        .unwrap_or(Value::Null)
}
fn render(s: &State, digest: &str, result: &mut OperationResult) {
    let obs = observation(s);
    let comparison = if s.steps[2].state == "completed" {
        s.steps[2]
            .receipt
            .as_ref()
            .map(|r| r["data"].clone())
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let instructions = if s.state == "waiting" {
        definition::installed()
            .ok()
            .map(|d| json!(d.steps[s.next].description))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    result.data = json!({"journal":{"id":s.run_id,"revision":s.revision,"digest":digest},"runbook":s.definition,"workflow_state":s.state,"in_flight":s.in_flight,"resume_safety":if s.in_flight.is_some(){"requires_fresh_attempt"}else if matches!(s.state.as_str(),"completed"|"failed"){"terminal"}else{"resumable"},"pending_checkpoint":if s.state=="waiting" {json!(s.steps[s.next].id)}else{Value::Null},"remaining_timeout_ms":s.remaining_timeout_ms,"observation_state":obs,"comparison":comparison,"checkpoint_instructions":instructions,"fresh_from":s.fresh_from,"steps":s.steps.iter().map(|step|json!({"id":step.id,"state":step.state,"condition":step.condition,"acknowledged_at":step.acknowledged_at,"receipt_summary":step.receipt.as_ref().map(|r|json!({"run_id":r["run_id"],"status":r["execution"]["status"],"started_at":r["started_at"],"finished_at":r["finished_at"],"coverage":r["coverage"],"errors":r["errors"]}))})).collect::<Vec<_>>()});
    result.coverage.state = if s.state == "completed" && obs == "complete" {
        "not_established"
    } else {
        "partial"
    }
    .into();
    result.coverage.scope =
        "Retained observations reviewed through an explicit operator checkpoint".into();
    result.coverage.limitations = vec![
        "Acknowledgement does not establish service health or complete telemetry coverage.".into(),
    ];
    result.execution.status = if s.state == "failed" {
        Status::Failed
    } else if s.state == "completed" && obs == "complete" {
        Status::Succeeded
    } else {
        Status::Partial
    };
}
fn halt_after(usable: bool, status: &Status, on_failure: &str, index: usize) -> bool {
    !usable
        && (matches!(
            status,
            Status::Denied | Status::Unsupported | Status::Cancelled | Status::TimedOut
        ) || on_failure == "stop"
            || index == 2)
}
fn comparison_ready(steps: &[StepState]) -> bool {
    steps[..2].iter().all(|s| s.state == "completed")
}
fn fail_remaining(s: &mut State) {
    s.state = "failed".into();
    for step in &mut s.steps[s.next..] {
        step.state = "skipped".into();
        step.condition = None;
    }
}
fn persist(
    store: &mut store::Store,
    s: &mut State,
    deadline: Instant,
    result: &mut OperationResult,
) -> Result<String, &'static str> {
    let old = s.revision;
    s.revision = s.revision.checked_add(1).ok_or("runbook_corrupt_state")?;
    s.updated_at = timestamp();
    s.remaining_timeout_ms = deadline
        .saturating_duration_since(Instant::now())
        .as_millis() as u64;
    validate(s)?;
    let digest = store
        .replace(&s.run_id, old, &serde_json::to_value(&s).unwrap())
        .map_err(|e| {
            if e.effects {
                result.effect_outcome = "unknown".into();
            }
            e.code
        })?;
    render(s, &digest, result);
    result.effect_outcome = "succeeded".into();
    Ok(digest)
}
fn execute(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) -> Result<(), &'static str> {
    definition::installed()?;
    let directory = context
        .runbook_state_dir
        .as_ref()
        .ok_or("runbook_state_directory_required")?;
    if !directory.is_absolute() {
        return Err("runbook_state_directory_absolute_required");
    }
    let invocation_deadline = start + Duration::from_millis(request.limits.timeout_ms);
    active(control, invocation_deadline)?;
    let is_new = request.operation == "runbook.run";
    let is_status = request.operation == "runbook.status";
    if is_new {
        exact(
            &request.inputs,
            &["id", "version", "input"],
            &["expected_definition_sha256", "fresh_from"],
        )?;
    } else if is_status {
        exact(&request.inputs, &["run_id"], &[])?;
    } else {
        exact(
            &request.inputs,
            &["run_id", "expected_revision", "input"],
            &["acknowledge"],
        )?;
    }
    let mut acknowledgement = None;
    let mut new_input = None;
    let mut fresh = None;
    let id = if is_new {
        if request.inputs["id"] != "evidence-review" || request.inputs["version"] != 1 {
            return Err("runbook_unknown_definition");
        }
        if let Some(pin) = request.inputs.get("expected_definition_sha256") {
            let pin = pin.as_str().filter(|p| digest(p)).ok_or_else(invalid)?;
            if pin != definition::SHA {
                return Err("runbook_definition_mismatch");
            }
        }
        new_input = Some(input(&request.inputs["input"])?);
        if let Some(v) = request.inputs.get("fresh_from") {
            exact(v, &["id", "expected_revision"], &[])?;
            let f: Fresh = serde_json::from_value(v.clone()).map_err(|_| invalid())?;
            if !crate::evidence::store::valid_run_id(&f.id) || f.expected_revision == 0 {
                return Err(invalid());
            }
            fresh = Some(f);
        }
        result.run_id.clone()
    } else {
        let id = request.inputs["run_id"]
            .as_str()
            .filter(|s| crate::evidence::store::valid_run_id(s))
            .ok_or_else(invalid)?
            .to_string();
        if !is_status {
            new_input = Some(input(&request.inputs["input"])?);
            if request.inputs["expected_revision"]
                .as_u64()
                .filter(|n| *n > 0)
                .is_none()
            {
                return Err(invalid());
            }
            if let Some(a) = request.inputs.get("acknowledge") {
                acknowledgement = Some(
                    a.as_str()
                        .filter(|a| {
                            matches!(
                                *a,
                                "review-complete" | "review-partial" | "review-incompatible"
                            )
                        })
                        .ok_or_else(invalid)?
                        .to_string(),
                );
            }
        }
        id
    };
    // Verify source data before initializing a new durable directory.
    let newly_pinned = if is_new {
        Some(pin(
            new_input.as_ref().unwrap(),
            context,
            control,
            invocation_deadline,
        )?)
    } else {
        None
    };
    if !is_status {
        result.record_mode = if request.record == "always" {
            "always"
        } else {
            "auto"
        };
    }
    let mut store = store::Store::open(
        directory,
        is_new && fresh.is_none(),
        invocation_deadline + Duration::from_millis(2500),
    )
    .map_err(|e| {
        if e.effects {
            result.effect_outcome = "unknown".into();
        }
        e.code
    })?;
    let (mut state, mut revision_digest) = if is_new {
        if let Some(f) = &fresh {
            let (v, _) = store.load(&f.id).map_err(|e| e.code)?;
            let old: State = serde_json::from_value(v).map_err(|_| "runbook_corrupt_state")?;
            validate(&old)?;
            if old.revision != f.expected_revision {
                return Err("runbook_revision_mismatch");
            }
            if old.input != *new_input.as_ref().unwrap()
                || serde_json::to_value(&old.target).unwrap()
                    != serde_json::to_value(&request.target).unwrap()
            {
                return Err("runbook_input_mismatch");
            }
        }
        active(control, invocation_deadline)?;
        let i = new_input.take().unwrap();
        let d = definition::installed()?;
        let now = timestamp();
        let s = State {
            format_version: 1,
            run_id: id.clone(),
            revision: 1,
            definition: identity(),
            target: request.target.clone(),
            input_sha256: input_hash(&i),
            input: i,
            pinned: newly_pinned.unwrap(),
            created_at: now.clone(),
            updated_at: now,
            state: "ready".into(),
            in_flight: None,
            next: 0,
            steps: d
                .steps
                .into_iter()
                .map(|s| StepState {
                    id: s.id,
                    state: "pending".into(),
                    receipt: None,
                    condition: None,
                    acknowledged_at: None,
                })
                .collect(),
            remaining_timeout_ms: invocation_deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u64,
            fresh_from: fresh,
        };
        let digest = store
            .create(&id, &serde_json::to_value(&s).unwrap())
            .map_err(|e| {
                if e.effects {
                    result.effect_outcome = "unknown".into();
                }
                e.code
            })?;
        result.effect_outcome = "succeeded".into();
        (s, digest)
    } else {
        let (v, digest) = store.load(&id).map_err(|e| e.code)?;
        let s: State = serde_json::from_value(v).map_err(|_| "runbook_corrupt_state")?;
        validate(&s)?;
        if s.run_id != id {
            return Err("runbook_corrupt_state");
        }
        if serde_json::to_value(&s.target).unwrap()
            != serde_json::to_value(&request.target).unwrap()
        {
            return Err("runbook_input_mismatch");
        }
        (s, digest)
    };
    render(&state, &revision_digest, result);
    if is_status {
        render(&state, &revision_digest, result);
        result.effect_outcome = "not_applicable".into();
        result.execution.status = Status::Succeeded;
        return Ok(());
    }
    result.record_mode = if request.record == "always" {
        "always"
    } else {
        "auto"
    };
    let deadline =
        invocation_deadline.min(start + Duration::from_millis(state.remaining_timeout_ms));
    if !is_new {
        if request.inputs["expected_revision"].as_u64() != Some(state.revision) {
            return Err("runbook_revision_mismatch");
        }
        if new_input.as_ref() != Some(&state.input) {
            return Err("runbook_input_mismatch");
        }
        if matches!(state.state.as_str(), "completed" | "failed") || state.in_flight.is_some() {
            return Err("runbook_resume_unsafe");
        }
        active(control, deadline)?;
        if pin(&state.pinned, context, control, deadline).map_err(revalidation_error)?
            != state.pinned
        {
            return Err("runbook_source_changed");
        }
        active(control, deadline)?;
        if state.state == "waiting" {
            if acknowledgement.as_deref() != Some(state.steps[state.next].id.as_str()) {
                return Err("runbook_checkpoint_mismatch");
            }
            state.steps[state.next].state = "acknowledged".into();
            state.steps[state.next].acknowledged_at = Some(timestamp());
            state.next += 1;
            state.state = "running".into();
            revision_digest = persist(&mut store, &mut state, deadline, result)?;
            result.effect_outcome = "succeeded".into();
        } else if acknowledgement.is_some() {
            return Err("runbook_checkpoint_mismatch");
        }
    }
    while state.next < 6 {
        if let Err(code) = active(control, deadline) {
            store.set_deadline(Instant::now() + Duration::from_millis(2500));
            state.state = "interrupted".into();
            revision_digest = persist(&mut store, &mut state, deadline, result)?;
            render(&state, &revision_digest, result);
            result.execution.status = if code == "deadline_exceeded" {
                Status::TimedOut
            } else {
                Status::Cancelled
            };
            result.error(
                code,
                "Runbook stopped; completed observations are retained and will not be repeated.",
            );
            result.data["cancellation_signal"] = json!(control.signal.load(Ordering::Relaxed));
            result.effect_outcome = "succeeded".into();
            return Ok(());
        }
        let n = state.next;
        if n >= 3 {
            let expected = ["complete", "partial", "incompatible"][n - 3];
            let matched = observation(&state) == expected;
            state.steps[n].condition = Some(matched);
            if matched {
                state.steps[n].state = "waiting".into();
                state.state = "waiting".into();
                revision_digest = persist(&mut store, &mut state, deadline, result)?;
                break;
            }
            state.steps[n].state = "skipped".into();
            state.next += 1;
            revision_digest = persist(&mut store, &mut state, deadline, result)?;
            continue;
        }
        if n == 2 && !comparison_ready(&state.steps) {
            state.steps[n].state = "skipped".into();
            state.next += 1;
            fail_remaining(&mut state);
            revision_digest = persist(&mut store, &mut state, deadline, result)?;
            break;
        }
        state.state = "running".into();
        state.in_flight = Some(state.steps[n].id.clone());
        persist(&mut store, &mut state, deadline, result)?;
        // Durable intent precedes dispatch. A crash here is deliberately uncertain.
        if let Err(code) = active(control, deadline) {
            store.set_deadline(Instant::now() + Duration::from_millis(2500));
            state.in_flight = None;
            state.state = "interrupted".into();
            persist(&mut store, &mut state, deadline, result)?;
            return Err(code);
        }
        let (operation, inputs) = match n {
            0 => (
                "evidence.show",
                serde_json::to_value(&state.pinned.before).unwrap(),
            ),
            1 => (
                "evidence.show",
                serde_json::to_value(&state.pinned.after).unwrap(),
            ),
            _ => (
                "evidence.compare",
                json!({"before":state.pinned.before,"after":state.pinned.after,"normalization":state.pinned.normalization}),
            ),
        };
        let mut child = Request::new(operation, inputs);
        child.target = request.target.clone();
        child.limits = request.limits.clone();
        child.limits.max_output_bytes = child.limits.max_output_bytes.min(256 * 1024);
        child.limits.max_items = if n == 2 { Some(100) } else { None };
        child.record = "never".into();
        let receipt = crate::execute_runbook_child(child, control, context, deadline);
        if active(control, deadline).is_err() {
            store.set_deadline(Instant::now() + Duration::from_millis(2500));
        }
        let usable = if n == 2 {
            matches!(
                receipt.execution.status,
                Status::Succeeded | Status::Partial
            ) && matches!(
                receipt.data["state"].as_str(),
                Some("complete" | "partial" | "incompatible")
            )
        } else {
            matches!(receipt.execution.status, Status::Succeeded)
        };
        let halt = halt_after(
            usable,
            &receipt.execution.status,
            &state.input.on_failure,
            n,
        );
        state.steps[n].receipt =
            Some(serde_json::to_value(receipt).map_err(|_| "runbook_receipt_serialization")?);
        state.steps[n].state = if usable { "completed" } else { "failed" }.into();
        state.in_flight = None;
        state.next += 1;
        if halt {
            fail_remaining(&mut state);
        }
        revision_digest = persist(&mut store, &mut state, deadline, result)?;
        result.effect_outcome = "succeeded".into();
        if state.state == "failed" {
            active(control, deadline)?;
            break;
        }
    }
    if state.next == 6 {
        state.state = "completed".into();
        revision_digest = persist(&mut store, &mut state, deadline, result)?;
    }
    render(&state, &revision_digest, result);
    Ok(())
}
pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    if let Err(code) = execute(request, control, context, start, result) {
        reject(result, code);
        if code == "deadline_exceeded" || code == "store_deadline_exceeded" {
            result.execution.status = Status::TimedOut;
        } else if code == "cancelled"
            || code == "store_cancelled"
            || code == "result_consumer_closed"
        {
            result.execution.status = Status::Cancelled;
        } else if result.effect_outcome == "succeeded" || result.effect_outcome == "unknown" {
            result.execution.status = Status::Failed;
        }
        if control.signal.load(Ordering::Relaxed) != 0 {
            if !result.data.is_object() {
                result.data = json!({});
            }
            result.data["cancellation_signal"] = json!(control.signal.load(Ordering::Relaxed));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_inputs() {
        let v = json!({"before":{"id":"run-1-ab-0"},"after":{"id":"run-1-ac-0"},"normalization":"exact-window-v1","on_failure":"stop"});
        assert!(input(&v).is_ok());
        for (key, value) in [
            ("normalization", json!("guess")),
            ("on_failure", Value::Null),
            ("extra", json!(true)),
        ] {
            let mut x = v.clone();
            x[key] = value;
            assert!(input(&x).is_err());
        }
        let mut x = v;
        x["before"]["expected_manifest_sha256"] = Value::Null;
        assert!(input(&x).is_err());
    }
    #[test]
    fn state_rejects_wrong_identity_branch_and_budget() {
        let input=input(&json!({"before":{"id":"run-1-ab-0"},"after":{"id":"run-1-ac-0"},"normalization":"exact-window-v1","on_failure":"stop"})).unwrap();
        let mut pinned = input.clone();
        pinned.before.expected_manifest_sha256 = Some(format!("sha256:{}", "0".repeat(64)));
        pinned.after.expected_manifest_sha256 = pinned.before.expected_manifest_sha256.clone();
        let state = State {
            format_version: 1,
            run_id: "run-1-ad-0".into(),
            revision: 1,
            definition: identity(),
            target: Target::default(),
            input_sha256: input_hash(&input),
            input,
            pinned,
            created_at: timestamp(),
            updated_at: timestamp(),
            state: "ready".into(),
            in_flight: None,
            next: 0,
            steps: definition::installed()
                .unwrap()
                .steps
                .into_iter()
                .map(|s| StepState {
                    id: s.id,
                    state: "pending".into(),
                    receipt: None,
                    condition: None,
                    acknowledged_at: None,
                })
                .collect(),
            remaining_timeout_ms: 30_000,
            fresh_from: None,
        };
        assert!(validate(&state).is_ok());
        let original = serde_json::to_value(state).unwrap();
        for (key, value) in [
            ("remaining_timeout_ms", json!(300001)),
            ("next", json!(7)),
            ("in_flight", json!("compare")),
            ("state", json!("completed")),
            ("input_sha256", json!("changed")),
            ("extra", json!(true)),
        ] {
            let mut v = original.clone();
            v[key] = value;
            assert!(
                serde_json::from_value::<State>(v)
                    .ok()
                    .and_then(|s| validate(&s).ok())
                    .is_none()
            );
        }
        let mut failed: State = serde_json::from_value(original.clone()).unwrap();
        let mut receipt = OperationResult::new(&Request::new("evidence.show", json!({})));
        receipt.execution.status = Status::Failed;
        failed.steps[0].state = "failed".into();
        failed.steps[0].receipt = Some(serde_json::to_value(receipt).unwrap());
        failed.next = 1;
        fail_remaining(&mut failed);
        assert!(validate(&failed).is_ok());
        assert!(failed.steps[1..].iter().all(|step| step.state == "skipped"));
        failed.steps[1].state = "pending".into();
        assert!(validate(&failed).is_err());
        let mut v = original;
        v["steps"][3]["condition"] = json!(true);
        assert!(validate(&serde_json::from_value(v).unwrap()).is_err());
    }
    #[test]
    fn failure_policy_stops_denial_and_never_bypasses_prerequisites() {
        assert!(!halt_after(false, &Status::Failed, "continue", 0));
        assert!(halt_after(false, &Status::Failed, "stop", 0));
        for status in [
            Status::Denied,
            Status::Unsupported,
            Status::Cancelled,
            Status::TimedOut,
        ] {
            assert!(halt_after(false, &status, "continue", 0));
        }
        assert!(!halt_after(true, &Status::Partial, "stop", 2));
        assert!(halt_after(false, &Status::Failed, "continue", 2));
        let mut steps = vec![
            StepState {
                id: "before".into(),
                state: "completed".into(),
                receipt: None,
                condition: None,
                acknowledged_at: None
            };
            2
        ];
        assert!(comparison_ready(&steps));
        for outcome in ["failed", "pending", "skipped"] {
            steps[0].state = outcome.into();
            assert!(!comparison_ready(&steps));
        }
    }
    #[test]
    fn source_revalidation_preserves_control_and_contention_failures() {
        for code in [
            "cancelled",
            "deadline_exceeded",
            "result_consumer_closed",
            "store_cancelled",
            "store_deadline_exceeded",
            "store_busy",
        ] {
            assert_eq!(revalidation_error(code), code);
        }
        for code in ["store_corrupt", "store_missing", "manifest_digest_mismatch"] {
            assert_eq!(revalidation_error(code), "runbook_source_changed");
        }
    }
    #[test]
    fn expired_budget_and_signal_stop() {
        let c = RunControl::default();
        assert_eq!(active(&c, Instant::now()), Err("deadline_exceeded"));
        c.signal.store(15, Ordering::Relaxed);
        assert_eq!(
            active(&c, Instant::now() + Duration::from_secs(1)),
            Err("cancelled")
        );
    }
}
