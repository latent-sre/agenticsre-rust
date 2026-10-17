use super::*;
use std::sync::{Arc, atomic::AtomicUsize, mpsc};
use store::Store;
const TRANSACTION: Duration = Duration::from_secs(5);
pub(super) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    run_scoped(request, control, context, start, result, None);
}
pub(super) fn run_scoped(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
    remote_scope: Option<&Value>,
) {
    result.record_mode = match request.record.as_str() {
        "auto" => "auto",
        "always" => "always",
        _ => "never",
    };
    result.data = api::initial();
    result.coverage.scope="One explicit local worker attempt and durable outcome; no implicit replay or remote ownership.".into();
    // A worker's output consumer is not the owner of the durable child. Its explicit
    // signals still cancel work, but disconnected watchers/output never do so.
    let worker_control = RunControl {
        signal: control.signal.clone(),
        result_fd: None,
    };
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    if let Err(error) = perform(
        request,
        &worker_control,
        context,
        deadline,
        result,
        remote_scope,
    ) {
        fail(error, &worker_control, result);
    }
    let signal = control.signal.load(Ordering::Relaxed);
    if signal != 0 {
        result.data["cancellation_signal"] = json!(signal);
    }
}
fn perform(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    deadline: Instant,
    result: &mut OperationResult,
    remote_scope: Option<&Value>,
) -> Result<(), Failure> {
    api::validate(request)?;
    active(control, deadline)?;
    let path = api::path(context)?;
    let store = Store::open(path, false, control, deadline)?;
    let mut job = store.load(request.inputs["job_id"].as_str().unwrap())?;
    if job.state.state == "terminal" {
        return api::respond(request, &store, &job, "terminal", result);
    }
    let _lease = store::claim(&job)?.ok_or_else(|| Failure::new("job_worker_busy"))?;
    if job.state.state != "queued" {
        if job.state.state == "finalizing" && store.result(&job)?.is_some() {
            api::terminal(&store, &mut job)?;
            result.effect_outcome = "succeeded".into();
            return api::respond(request, &store, &job, "terminal", result);
        }
        if job.state.state != "recovery_required" {
            job.state.state = "recovery_required".into();
            job.state.execution_status = Some("unknown".into());
            job.state.effect_outcome = "unknown".into();
            job.state
                .event("recovery_required", None, job.admission.retain_events);
            store.commit(&job)?;
            result.effect_outcome = "succeeded".into();
        }
        api::respond(request, &store, &job, "recovery_required", result)?;
        return Err(Failure::new("job_recovery_required"));
    }
    if job.admission.max_runtime_ms > api::ceiling(context)? {
        return Err(Failure::new("job_runtime_denied"));
    }
    job.state.state = "running".into();
    job.state.dispatched_at = Some(crate::result::timestamp());
    job.state.attempt_id = Some(crate::result::new_id("attempt"));
    job.state.effect_outcome = "unknown".into();
    job.state
        .event("running", Some(0), job.admission.retain_events);
    store.commit(&job)?;
    result.effect_outcome = "unknown".into();
    result.effective_limits.timeout_ms = job.admission.max_runtime_ms;
    let admission = job.admission.clone();
    let mut last_state = job.state.clone();
    let location = store.location.clone();
    drop(store);
    let signal = Arc::new(AtomicUsize::new(0));
    let child_control = RunControl {
        signal: signal.clone(),
        result_fd: None,
    };
    let started = Instant::now();
    let mut last_event = started;
    let mut persistence_failure = None;
    let child = std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::sync_channel(1);
        let admission_ref = &admission;
        let child_control_ref = &child_control;
        let executor = scope.spawn(move || {
            let child = if admission_ref.implementation["kind"] == "remote" {
                super::remote::execute(admission_ref, child_control_ref, context, remote_scope)
            } else {
                dispatch::execute(admission_ref, child_control_ref, context)
            };
            let _ = sender.send(child);
        });
        let mut child = None;
        loop {
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(value) => {
                    child = Some(value);
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let requested = control.signal.load(Ordering::Relaxed);
            if requested != 0 {
                signal.store(requested, Ordering::Relaxed);
            }
            if persistence_failure.is_some() {
                continue;
            }
            let observation = (|| {
                let durable_control = RunControl::default();
                let store =
                    Store::open(path, false, &durable_control, Instant::now() + TRANSACTION)?;
                let mut current = store.load(&admission.job_id)?;
                if current.state.attempt_id != last_state.attempt_id
                    || !matches!(current.state.state.as_str(), "running" | "cancelling")
                {
                    return Err(Failure::new("job_ownership_changed"));
                }
                if current.state.cancel_requested && requested == 0 {
                    signal.store(15, Ordering::Relaxed);
                }
                let mut changed = false;
                if requested != 0 && !current.state.cancel_requested {
                    current.state.cancel_requested = true;
                    current.state.state = "cancelling".into();
                    current.state.event(
                        "cancel_requested",
                        Some(started.elapsed().as_millis() as u64),
                        admission.retain_events,
                    );
                    changed = true;
                }
                if last_event.elapsed() >= Duration::from_secs(1) {
                    current.state.event(
                        "heartbeat",
                        Some(started.elapsed().as_millis() as u64),
                        admission.retain_events,
                    );
                    last_event = Instant::now();
                    changed = true;
                }
                if changed {
                    store.commit(&current)?;
                }
                last_state = current.state;
                Ok(())
            })();
            if let Err(error) = observation {
                persistence_failure = Some(error);
                if requested == 0 {
                    signal.store(15, Ordering::Relaxed);
                }
            }
        }
        if executor.join().is_err() {
            persistence_failure = Some(Failure::new("job_executor_interrupted"));
        }
        child
    });
    let Some(child) = child else {
        result.data["job"] =
            api::description(&admission, &last_state, &location, "recovery_required");
        result.data["action"] = json!("recovery_required");
        return Err(Failure::new("job_recovery_required").after_effects());
    };
    // Keep the actual child receipt available even if a late persistence failure
    // prevents committing a durable terminal. It is never relabelled as unexecuted.
    if let Some(error) = persistence_failure {
        result.data["job"] =
            api::description(&admission, &last_state, &location, "recovery_required");
        result.data["action"] = json!("recovery_required");
        api::failed_child_detail(result, &child);
        return Err(error.after_effects());
    }
    let finalized = (|| {
        let durable_control = RunControl::default();
        let store = Store::open(path, false, &durable_control, Instant::now() + TRANSACTION)?;
        let mut current = store.load(&admission.job_id)?;
        if current.state.attempt_id != last_state.attempt_id
            || !matches!(current.state.state.as_str(), "running" | "cancelling")
        {
            return Err(Failure::new("job_ownership_changed"));
        }
        api::finalize(&store, &mut current, &child)?;
        api::respond(request, &store, &current, "worked", result)
    })();
    if let Err(error) = finalized {
        result.data["job"] =
            api::description(&admission, &last_state, &location, "recovery_required");
        result.data["action"] = json!("recovery_required");
        api::failed_child_detail(result, &child);
        return Err(error.after_effects());
    }
    Ok(())
}
