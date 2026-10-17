use super::{
    store::{Store, active, error},
    *,
};
use crate::{
    evidence::store::StoreError,
    result::{Status, new_id, timestamp},
    tasks,
};
use std::{path::Path, sync::atomic::Ordering, time::Duration};
fn failure(result: &mut OperationResult, error: StoreError, control: &RunControl) {
    result.execution.status = match error.code {
        "toil_cancelled" | "toil_result_consumer_closed" => Status::Cancelled,
        "toil_deadline_exceeded" => Status::TimedOut,
        "toil_disabled" | "toil_expired_sample" | "toil_future_sample" => Status::Denied,
        _ => Status::Failed,
    };
    if error.effects {
        result.effect_outcome = "unknown".into();
    }
    result.error(error.code,"The private toil metadata operation did not complete; inspect recording and any child receipt before retrying.");
    let signal = control.signal.load(Ordering::Relaxed);
    if signal != 0 {
        result.data["cancellation_signal"] = json!(signal);
    }
}
fn imported(input: &Value) -> Sample {
    let elapsed = input["elapsed_ms"].as_u64();
    let effort = input["effort_ms"].as_u64();
    Sample {
        id: new_id("sample"),
        workflow: input["workflow"].as_str().unwrap().into(),
        cohort: input["cohort"].as_str().unwrap().into(),
        observed_at: input["observed_at"].as_str().unwrap().into(),
        elapsed_ms: elapsed,
        elapsed_measurement: elapsed.map(|_| input["measurement"].as_str().unwrap().into()),
        effort_ms: effort,
        effort_measurement: effort.map(|_| input["measurement"].as_str().unwrap().into()),
        effort_provenance: effort.map(|_| "operator_reported".into()),
        outcome: input["outcome"].as_str().unwrap().into(),
        retries: input["retries"].as_u64(),
        manual_interventions: input["manual_interventions"].as_u64(),
        source: "operator_reported".into(),
        task: None,
        operation_run_id: None,
        annotated_at: None,
    }
}
pub(super) fn run(
    request: &Request,
    control: &RunControl,
    path: &Path,
    start: Instant,
    result: &mut OperationResult,
) {
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    let write = request.operation != "toil.summary";
    if write {
        result.record_mode = if request.record == "always" {
            "always"
        } else {
            "auto"
        };
    }
    let mut store = match Store::open(
        path,
        request.operation == "toil.configure",
        control,
        deadline,
    ) {
        Ok(s) => s,
        Err(e) => {
            failure(result, e, control);
            return;
        }
    };
    if matches!(
        request.operation.as_str(),
        "toil.record" | "toil.capture" | "toil.annotate"
    ) && let Err(e) = store.ensure_consent()
    {
        failure(result, e, control);
        return;
    }
    if matches!(request.operation.as_str(), "toil.record" | "toil.capture")
        && let Err(e) = store.ensure_capacity()
    {
        failure(result, e, control);
        return;
    }
    if request.operation == "toil.capture" {
        capture(request, control, deadline, &mut store, result);
        return;
    }
    let now = OffsetDateTime::now_utc();
    let outcome = (|| -> Result<Value, StoreError> {
        active(control, deadline)?;
        let input = &request.inputs;
        match request.operation.as_str() {
            "toil.configure" => {
                store.state.enabled = input["enabled"].as_bool().unwrap();
                store.state.retention_days = input["retention_days"].as_u64().unwrap();
                store.commit(control, deadline)?;
                Ok(
                    json!({"format_version":1,"configuration":{"enabled":store.state.enabled,"retention_days":store.state.retention_days},"samples_retained":store.state.samples.len()}),
                )
            }
            "toil.record" => {
                let observed = utc(&input["observed_at"]).unwrap();
                if observed > now {
                    return Err(error("toil_future_sample"));
                }
                if observed < now - time::Duration::days(store.state.retention_days as i64) {
                    return Err(error("toil_expired_sample"));
                }
                let sample = imported(input);
                store.state.samples.push(sample.clone());
                store.commit(control, deadline)?;
                Ok(
                    json!({"format_version":1,"sample":sample,"recording":{"state":"complete","sample_id":sample.id}}),
                )
            }
            "toil.annotate" => {
                let sample = store
                    .state
                    .samples
                    .iter_mut()
                    .find(|sample| Some(sample.id.as_str()) == input["id"].as_str())
                    .ok_or(error("toil_sample_not_found"))?;
                sample.effort_ms = input["effort_ms"].as_u64();
                sample.effort_measurement = Some(input["measurement"].as_str().unwrap().into());
                sample.effort_provenance = Some("operator_annotated".into());
                sample.annotated_at = Some(timestamp());
                if input.get("retries").is_some() {
                    sample.retries = input["retries"].as_u64();
                }
                if input.get("manual_interventions").is_some() {
                    sample.manual_interventions = input["manual_interventions"].as_u64();
                }
                let sample = sample.clone();
                store.commit(control, deadline)?;
                Ok(
                    json!({"format_version":1,"sample":sample,"recording":{"state":"complete","sample_id":sample.id}}),
                )
            }
            "toil.delete" | "toil.prune" => {
                let cutoff = now - time::Duration::days(store.state.retention_days as i64);
                let mut deleted = Vec::new();
                for sample in &store.state.samples {
                    active(control, deadline)?;
                    if if request.operation == "toil.delete" {
                        Some(sample.id.as_str()) == input["id"].as_str()
                    } else {
                        utc(&json!(sample.observed_at)).unwrap() < cutoff
                    } {
                        deleted.push(sample.id.clone());
                    }
                }
                if request.operation == "toil.delete" && deleted.is_empty() {
                    return Err(error("toil_sample_not_found"));
                }
                store
                    .state
                    .samples
                    .retain(|sample| !deleted.contains(&sample.id));
                store.commit(control, deadline)?;
                Ok(
                    json!({"format_version":1,"deleted":deleted,"remaining":store.state.samples.len()}),
                )
            }
            "toil.summary" => summary::build(
                input,
                &store.state.samples,
                store.state.enabled,
                store.state.retention_days,
                now,
                &mut || active(control, deadline).map_err(|e| e.code),
            )
            .map_err(error),
            _ => unreachable!("validated operation"),
        }
    })();
    match outcome {
        Ok(data) => {
            result.data = data;
            result.effect_outcome = if write { "succeeded" } else { "not_applicable" }.into();
            result.coverage.state = "not_established".into();
            result.coverage.scope="Opt-in workflow metadata only; no employee ranking, external telemetry or proven causal savings.".into();
            if let Some(limitations) = result.data["limitations"].as_array() {
                result.coverage.limitations = limitations
                    .iter()
                    .map(|v| v.as_str().unwrap().into())
                    .collect();
            }
            if let Err(e) = active(control, deadline) {
                failure(result, e, control);
            }
        }
        Err(e) => failure(
            result,
            if store.created_effects {
                e.after_effects()
            } else {
                e
            },
            control,
        ),
    }
}
fn capture(
    request: &Request,
    control: &RunControl,
    deadline: Instant,
    store: &mut Store,
    result: &mut OperationResult,
) {
    if let Err(e) = active(control, deadline) {
        failure(result, e, control);
        return;
    }
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .as_millis();
    if remaining == 0 {
        failure(result, error("toil_deadline_exceeded"), control);
        return;
    }
    let mut child_request = Request::new("task.run", request.inputs["task"].clone());
    child_request.limits.timeout_ms = remaining.min(300_000) as u64;
    child_request.limits.max_output_bytes = request.limits.max_output_bytes.min(128 * 1024);
    let child_start = Instant::now();
    let mut child = OperationResult::new(&child_request);
    tasks::run(&child_request, control, &mut child);
    child.finish(child_start);
    let sample = Sample {
        id: new_id("sample"),
        workflow: request.inputs["workflow"].as_str().unwrap().into(),
        cohort: request.inputs["cohort"].as_str().unwrap().into(),
        observed_at: child.started_at.clone(),
        elapsed_ms: Some(child.duration_ms),
        elapsed_measurement: Some("measured".into()),
        effort_ms: None,
        effort_measurement: None,
        effort_provenance: None,
        outcome: serde_json::to_value(&child.execution.status)
            .unwrap()
            .as_str()
            .unwrap()
            .into(),
        retries: Some(0),
        manual_interventions: None,
        source: "task_capture".into(),
        task: Some(
            json!({"id":request.inputs["task"]["id"],"version":request.inputs["task"]["version"]}),
        ),
        operation_run_id: Some(child.run_id.clone()),
        annotated_at: None,
    };
    let child_status = child.execution.status.clone();
    let mut payload = serde_json::to_value(child).expect("child receipt");
    let omitted = serde_json::to_vec(&payload).unwrap().len() > 256 * 1024;
    if omitted {
        payload["data"] = json!({});
        payload["output"]["stdout"] = json!("");
        payload["output"]["stderr"] = json!("");
        payload["coverage"]["state"] = json!("partial");
        payload["coverage"]["limitations"].as_array_mut().unwrap().push(json!("Child payload was omitted from the capture wrapper size bound; execution identity and status remain original."));
    }
    result.data = json!({"format_version":1,"child":payload,"child_payload_omitted":omitted,"sample":null,"recording":{"state":"not_attempted","sample_id":null}});
    result.execution.status = child_status;
    result.effect_outcome = "not_attempted".into();
    store.state.samples.push(sample.clone());
    match store.commit(control, deadline) {
        Ok(()) => {
            result.data["sample"] = json!(sample);
            result.data["recording"] = json!({"state":"complete","sample_id":sample.id});
            result.effect_outcome = "succeeded".into();
            if omitted && result.execution.status == Status::Succeeded {
                result.execution.status = Status::Partial;
            }
            if let Err(e) = active(control, deadline) {
                failure(result, e, control);
            }
        }
        Err(e) => {
            result.data["recording"]["state"] = json!(if e.effects { "unknown" } else { "failed" });
            if e.effects {
                result.data["recording"]["sample_id"] = json!(sample.id);
            }
            result.effect_outcome = if e.effects { "unknown" } else { "failed" }.into();
            failure(result, e, control);
        }
    }
    result.coverage.state = if omitted || result.data["recording"]["state"] != "complete" {
        "partial"
    } else {
        "not_established"
    }
    .into();
    result.coverage.scope="One installed task attempt; elapsed runtime is measured, human effort and interventions remain unknown until explicit annotation.".into();
    result.coverage.limitations.push("Only task identity, run id and bounded operational counters are persisted; task arguments, paths, output and findings are not retained by the toil store.".into());
}
