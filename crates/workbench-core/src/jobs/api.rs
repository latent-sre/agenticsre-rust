use super::*;
use model::{Admission, State};
use store::{Job, Store};
pub(super) fn validate(request: &Request) -> Result<(), Failure> {
    let input = &request.inputs;
    let valid = match request.operation.as_str() {
        "job.submit" => {
            keys(input, &["request", "max_runtime_ms", "retain_events"])
                && input["request"].is_object()
                && input["max_runtime_ms"]
                    .as_u64()
                    .is_some_and(|n| (1..=model::MAX_RUNTIME).contains(&n))
                && input["retain_events"]
                    .as_u64()
                    .is_some_and(|n| (1..=256).contains(&n))
        }
        "job.status" | "job.cancel" | "job.work" => {
            keys(input, &["job_id"]) && input["job_id"].as_str().is_some_and(model::valid_id)
        }
        "job.watch" => {
            keys(input, &["job_id", "after_sequence"])
                && input["job_id"].as_str().is_some_and(model::valid_id)
                && input["after_sequence"]
                    .as_u64()
                    .is_some_and(|n| n <= 9_007_199_254_740_991)
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(Failure::new("invalid_job_input"))
    }
}
pub(super) fn path(context: &OperatorContext) -> Result<&std::path::Path, Failure> {
    context
        .job_state_dir
        .as_deref()
        .ok_or_else(|| Failure::new("job_store_required"))
}
pub(super) fn ceiling(context: &OperatorContext) -> Result<u64, Failure> {
    let ceiling = context.job_max_runtime_ms.unwrap_or(model::MAX_RUNTIME);
    if (1..=model::MAX_RUNTIME).contains(&ceiling) {
        Ok(ceiling)
    } else {
        Err(Failure::new("invalid_job_runtime_policy"))
    }
}
pub(super) fn initial() -> Value {
    json!({"format_version":1,"action":"status","job":null,"events":[],"cursor":{"after_sequence":0,"first_available_sequence":0,"last_sequence":0,"next_sequence":0,"gap":null,"has_more":false},"result":null,"result_sha256":null,"result_omitted":false,"cancellation_signal":null})
}
pub(super) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    result.record_mode = match request.record.as_str() {
        "auto" => "auto",
        "always" => "always",
        _ => "never",
    };
    result.data = initial();
    result.coverage.scope="Durable local job state and bounded retained events; child outcome is separate from management success.".into();
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    if let Err(error) = perform(request, control, context, deadline, result) {
        fail(error, control, result);
    }
}
fn perform(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    deadline: Instant,
    result: &mut OperationResult,
) -> Result<(), Failure> {
    validate(request)?;
    active(control, deadline)?;
    if request.operation == "job.work" {
        return Err(Failure::new("job_worker_entry_required"));
    }
    if request.operation == "job.submit" {
        let value = &request.inputs["request"];
        if !["target", "limits", "inputs"]
            .iter()
            .all(|key| value[*key].is_object())
        {
            return Err(Failure::new("invalid_job_input"));
        }
        let nested_bytes = serde_json::to_vec(value).expect("nested request");
        let mut nested = crate::request::parse_request(nested_bytes.as_slice())
            .map_err(|p| Failure::new(p.code))?;
        if nested.record != "never"
            || serde_json::to_vec(&nested).expect("request").len() > 64 * 1024
        {
            return Err(Failure::new("invalid_job_input"));
        }
        let runtime = request.inputs["max_runtime_ms"].as_u64().unwrap();
        if runtime > ceiling(context)? {
            return Err(Failure::new("job_runtime_denied"));
        }
        let retention = request.inputs["retain_events"].as_u64().unwrap() as usize;
        let implementation = dispatch::admit(&mut nested, control, context, deadline)?;
        let fingerprint = model::fingerprint(&nested, runtime, retention, &implementation, uid());
        let store = Store::open(path(context)?, true, control, deadline)?;
        if store.created {
            result.effect_outcome = "succeeded".into();
        }
        for id in store.ids()? {
            let job = store.load(&id)?;
            if job.admission.owner_uid == uid()
                && job.admission.request.request_id == nested.request_id
            {
                if job.admission.request_sha256 != fingerprint {
                    return Err(Failure::new("job_request_conflict"));
                }
                return respond(request, &store, &job, "existing", result);
            }
        }
        let admission = Admission {
            format_version: 1,
            job_id: crate::result::new_id("job"),
            run_id: crate::result::new_id("run"),
            owner_uid: uid(),
            request: nested,
            request_sha256: fingerprint,
            max_runtime_ms: runtime,
            retain_events: retention,
            created_at: crate::result::timestamp(),
            implementation,
        };
        let job = store.create(admission)?;
        result.effect_outcome = "succeeded".into();
        return respond(request, &store, &job, "submitted", result);
    }
    let store = Store::open(path(context)?, false, control, deadline)?;
    let mut job = store.load(request.inputs["job_id"].as_str().unwrap())?;
    let action = if request.operation == "job.cancel" {
        if job.state.state == "terminal" {
            "terminal"
        } else if job.state.state == "queued" {
            job.state.cancel_requested = true;
            let child = dispatch::cancelled(&job.admission);
            finalize(&store, &mut job, &child)?;
            result.effect_outcome = "succeeded".into();
            "cancelled"
        } else {
            if !job.state.cancel_requested {
                job.state.cancel_requested = true;
                if job.state.state == "running" {
                    job.state.state = "cancelling".into();
                }
                job.state
                    .event("cancel_requested", None, job.admission.retain_events);
                store.commit(&job)?;
                result.effect_outcome = "succeeded".into();
            }
            "cancel_requested"
        }
    } else if request.operation == "job.watch" {
        "watch"
    } else {
        "status"
    };
    respond(request, &store, &job, action, result)
}
pub(super) fn finalize(
    store: &Store,
    job: &mut Job,
    child: &OperationResult,
) -> Result<(), Failure> {
    let bytes = serde_json::to_vec(child).expect("child result");
    if bytes.len() > model::MAX_RESULT {
        return Err(Failure::new("job_result_too_large"));
    }
    job.state.state = "finalizing".into();
    job.state.result_sha256 = Some(model::digest(&bytes));
    job.state.execution_status = Some(
        serde_json::to_value(&child.execution.status)
            .unwrap()
            .as_str()
            .unwrap()
            .into(),
    );
    job.state.effect_outcome = child.effect_outcome.clone();
    job.state
        .event("finalizing", None, job.admission.retain_events);
    store.commit(job)?;
    #[cfg(test)]
    if FAIL_AFTER_FINALIZING.with(|flag| flag.replace(false)) {
        return Err(Failure::new("job_deadline_exceeded").after_effects());
    }
    store
        .publish_result(job, &bytes)
        .map_err(Failure::after_effects)?;
    terminal(store, job).map_err(Failure::after_effects)
}
pub(super) fn terminal(store: &Store, job: &mut Job) -> Result<(), Failure> {
    job.state.state = "terminal".into();
    job.state.finished_at = Some(crate::result::timestamp());
    job.state
        .event("terminal", None, job.admission.retain_events);
    store.commit(job)
}
pub(super) fn respond(
    request: &Request,
    store: &Store,
    job: &Job,
    action: &str,
    result: &mut OperationResult,
) -> Result<(), Failure> {
    let child = if job.state.state == "terminal" {
        Some(
            store
                .result(job)?
                .ok_or_else(|| Failure::new("job_store_recovery_required"))?,
        )
    } else {
        None
    };
    let orphan = matches!(
        job.state.state.as_str(),
        "running" | "cancelling" | "finalizing"
    ) && store::claim(job)?.is_some();
    let observed = if orphan {
        "recovery_required"
    } else {
        &job.state.state
    };
    let first = job.state.events[0].sequence;
    let after = if request.operation == "job.watch" {
        request.inputs["after_sequence"].as_u64().unwrap()
    } else {
        job.state.last_sequence
    };
    if after > job.state.last_sequence {
        return Err(Failure::new("invalid_job_cursor"));
    }
    let gap = if after < first.saturating_sub(1) {
        json!({"from_sequence":after+1,"to_sequence":first-1})
    } else {
        Value::Null
    };
    let mut events: Vec<Value> = if request.operation == "job.watch" {
        job.state
            .events
            .iter()
            .filter(|event| event.sequence > after)
            .take(64)
            .map(|e| serde_json::to_value(e).unwrap())
            .collect()
    } else {
        vec![]
    };
    let mut child = child;
    let mut omitted = false;
    let budget = result.effective_limits.max_output_bytes;
    if serde_json::to_vec(&json!({"events":events,"result":child}))
        .unwrap()
        .len()
        > budget
        && child.is_some()
    {
        child = None;
        omitted = true;
    }
    while serde_json::to_vec(&json!({"events":events,"result":child}))
        .unwrap()
        .len()
        > budget
    {
        if events.pop().is_none() {
            break;
        }
    }
    let next = events
        .last()
        .and_then(|e| e["sequence"].as_u64())
        .unwrap_or(after.max(first.saturating_sub(1)));
    result.data = json!({"format_version":1,"action":action,"job":description(&job.admission,&job.state,&store.location,observed),
        "events":events,"cursor":{"after_sequence":after,"first_available_sequence":first,"last_sequence":job.state.last_sequence,"next_sequence":next,"gap":gap,"has_more":next<job.state.last_sequence},"result":child,"result_sha256":job.state.result_sha256,"result_omitted":omitted,"cancellation_signal":null});
    if result.effect_outcome == "not_attempted" {
        result.effect_outcome = "not_applicable".into();
    }
    result.coverage.state = if omitted { "partial" } else { "complete" }.into();
    if omitted {
        result.execution.status = Status::Partial;
        result.coverage.limitations.push("Stored terminal result omitted by the response detail budget; use status with a larger budget. The immutable result digest is retained.".into());
    }
    Ok(())
}

pub(super) fn description(
    admission: &Admission,
    state: &State,
    location: &Value,
    observed: &str,
) -> Value {
    json!({"job_id":admission.job_id,"run_id":admission.run_id,"request_id":admission.request.request_id,"owner":{"kind":"local_uid","uid":admission.owner_uid},"execution_location":location,"request_sha256":admission.request_sha256,"operation":admission.request.operation,"operation_version":1,"max_runtime_ms":admission.max_runtime_ms,"retain_events":admission.retain_events,"state":observed,"stored_state":state.state,"created_at":admission.created_at,"dispatched_at":state.dispatched_at,"finished_at":state.finished_at,"attempt_id":state.attempt_id,"cancel_requested":state.cancel_requested,"last_sequence":state.last_sequence,"execution_status":if observed=="recovery_required"{Some("unknown")}else{state.execution_status.as_deref()},"effect_outcome":if observed=="recovery_required"{"unknown"}else{&state.effect_outcome}})
}

#[cfg(test)]
thread_local! {pub(super) static FAIL_AFTER_FINALIZING:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}

pub(super) fn failed_child_detail(result: &mut OperationResult, child: &OperationResult) {
    let bytes = serde_json::to_vec(child).expect("child receipt");
    let value = serde_json::to_value(child).expect("child receipt");
    result.data["result_sha256"] = json!(model::digest(&bytes));
    if serde_json::to_vec(&json!({"events":[],"result":value}))
        .expect("details")
        .len()
        > result.effective_limits.max_output_bytes
    {
        result.data["result"] = Value::Null;
        result.data["result_omitted"] = json!(true);
    } else {
        result.data["result"] = value;
    }
    result.coverage.limitations.push("Persistence failed after child execution. The receipt digest identifies the observed child result; terminal durability is unconfirmed, and stored_state is the last verified state.".into());
}
