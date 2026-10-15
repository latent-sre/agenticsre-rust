//! Durable local job ownership and state. No background service or implicit retry.
#[cfg(target_os = "linux")]
mod api;
#[cfg(target_os = "linux")]
mod dispatch;
#[cfg(target_os = "linux")]
mod model;
#[cfg(target_os = "linux")]
pub(crate) mod remote;
#[cfg(target_os = "linux")]
mod store;
#[cfg(target_os = "linux")]
mod worker;
use crate::{
    OperatorContext, RunControl,
    request::{Problem, Request},
    result::{OperationResult, Status},
};
use serde_json::{Value, json};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
#[derive(Debug)]
struct Failure {
    code: &'static str,
    effects: bool,
}
impl Failure {
    fn new(code: &'static str) -> Self {
        Self {
            code,
            effects: false,
        }
    }
    fn after_effects(mut self) -> Self {
        self.effects = true;
        self
    }
}
fn keys(value: &Value, required: &[&str]) -> bool {
    value.as_object().is_some_and(|map| {
        map.len() == required.len() && required.iter().all(|key| map.contains_key(*key))
    })
}
fn supported(operation: &str) -> bool {
    matches!(operation, "process.exec" | "task.run") || operation.starts_with("ext.")
}
#[cfg(target_os = "linux")]
fn uid() -> u32 {
    // SAFETY: geteuid has no pointer parameters and observes the current effective owner.
    unsafe { libc::geteuid() }
}
fn active(control: &RunControl, deadline: Instant) -> Result<(), Failure> {
    if control.signal.load(Ordering::Relaxed) != 0 {
        return Err(Failure::new("job_cancelled"));
    }
    if Instant::now() >= deadline {
        return Err(Failure::new("job_deadline_exceeded"));
    }
    #[cfg(target_os = "linux")]
    if let Some(fd) = control.result_fd {
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: initialized stack pollfd is exclusively borrowed for a nonblocking observation.
        if unsafe { libc::poll(&mut poll, 1, 0) } > 0
            && poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
        {
            return Err(Failure::new("job_consumer_closed"));
        }
    }
    Ok(())
}

fn fail(error: Failure, control: &RunControl, result: &mut OperationResult) {
    result.execution.status = match error.code {
        "job_cancelled" | "job_consumer_closed" => Status::Cancelled,
        "job_deadline_exceeded" => Status::TimedOut,
        "job_recovery_required" => Status::Unknown,
        "job_store_required"
        | "job_worker_entry_required"
        | "unsupported_job_operation"
        | "job_read_policy_unsupported" => Status::Unsupported,
        "job_request_conflict"
        | "job_worker_busy"
        | "invalid_job_input"
        | "invalid_job_cursor"
        | "job_runtime_denied"
        | "invalid_job_runtime_policy" => Status::Denied,
        code if code.starts_with("extension_") || code.starts_with("invalid_") => Status::Denied,
        _ => Status::Failed,
    };
    if error.effects {
        result.effect_outcome = "unknown".into();
    }
    result.coverage.state = "not_established".into();
    result.error(error.code,if error.effects{"Durable job updates may be incomplete; preserve stored bytes and reconcile explicitly. No automatic replay is authorized."}else{"The requested durable job action could not be established; no automatic replay is authorized."});
    let signal = control.signal.load(Ordering::Relaxed);
    if signal != 0 {
        result.data["cancellation_signal"] = json!(signal);
    }
}
pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    #[cfg(target_os = "linux")]
    api::run(request, control, context, start, result);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (request, context, start);
        result.reject(Problem::unsupported(
            "unsupported_platform",
            "Durable local jobs currently require Linux.",
        ));
    }
}
pub(crate) fn work(
    request: Request,
    control: &RunControl,
    context: &OperatorContext,
) -> OperationResult {
    let start = Instant::now();
    let mut result = OperationResult::new(&request);
    if request.operation != "job.work" {
        result.reject(Problem::invalid(
            "invalid_job_input",
            "The worker entry accepts only job.work.",
        ));
    } else if let Err(problem) = request.validate() {
        result.reject(problem);
    } else {
        #[cfg(target_os = "linux")]
        worker::run(&request, control, context, start, &mut result);
        #[cfg(not(target_os = "linux"))]
        result.reject(Problem::unsupported(
            "unsupported_platform",
            "Durable local jobs currently require Linux.",
        ));
    }
    result.finish(start);
    result
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use model::{Admission, State};
    use store::Store;
    fn admission() -> Admission {
        let request = Request::new(
            "process.exec",
            json!({"program":"/usr/bin/printf","args":["synthetic"],"cwd":"/"}),
        );
        let implementation = json!({"kind":"test"});
        Admission {
            format_version: 1,
            job_id: crate::result::new_id("job"),
            run_id: crate::result::new_id("run"),
            owner_uid: uid(),
            request_sha256: model::fingerprint(&request, 1000, 2, &implementation, uid()),
            request,
            max_runtime_ms: 1000,
            retain_events: 2,
            created_at: crate::result::timestamp(),
            implementation,
        }
    }
    #[test]
    fn immutable_admission_event_retention_and_private_lease_roundtrip() {
        use std::os::unix::fs::MetadataExt;
        let path = std::env::temp_dir().join(crate::result::new_id("job-store-test"));
        let control = RunControl::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        let store = Store::open(&path, true, &control, deadline).unwrap();
        let mut job = store.create(admission()).unwrap();
        let lease = store::claim(&job).unwrap().unwrap();
        assert!(store::claim(&job).unwrap().is_none());
        job.state.state = "running".into();
        job.state.attempt_id = Some(crate::result::new_id("attempt"));
        job.state.dispatched_at = Some(crate::result::timestamp());
        job.state.effect_outcome = "unknown".into();
        job.state
            .event("running", None, job.admission.retain_events);
        for _ in 0..3 {
            job.state
                .event("heartbeat", None, job.admission.retain_events);
        }
        store.commit(&job).unwrap();
        let loaded = store.load(&job.admission.job_id).unwrap();
        assert_eq!(loaded.state.events.len(), 2);
        assert_eq!(loaded.state.events[0].sequence, 4);
        assert_eq!(loaded.state.last_sequence, 5);
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o7777, 0o700);
        assert_eq!(
            std::fs::metadata(path.join(&job.admission.job_id).join("request.json"))
                .unwrap()
                .mode()
                & 0o7777,
            0o600
        );
        assert_eq!(
            loaded.admission.request_sha256,
            job.admission.request_sha256
        );
        drop(lease);
        assert!(store::claim(&job).unwrap().is_some());
        drop(store);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn malicious_persisted_event_sequence_is_rejected_without_overflow() {
        let mut admission = admission();
        admission.retain_events = 3;
        admission.request_sha256 = model::fingerprint(
            &admission.request,
            admission.max_runtime_ms,
            3,
            &admission.implementation,
            uid(),
        );
        let mut state = State::new(&admission);
        state.state = "running".into();
        state.dispatched_at = Some(crate::result::timestamp());
        state.attempt_id = Some(crate::result::new_id("attempt"));
        state.effect_outcome = "unknown".into();
        state.event("running", None, admission.retain_events);
        state.event("heartbeat", Some(1), admission.retain_events);
        assert!(
            state.valid(&admission),
            "baseline must be a valid active state"
        );
        state.events[0].sequence = u64::MAX;
        state.events[1].sequence = 0;
        state.events[2].sequence = 1;
        state.last_sequence = 1;
        assert!(!state.valid(&admission));
        let path = std::env::temp_dir().join(crate::result::new_id("job-corrupt-test"));
        let control = RunControl::default();
        let store = Store::open(
            &path,
            true,
            &control,
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
        let job = store.create(admission).unwrap();
        std::fs::write(
            path.join(&job.admission.job_id).join("state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        assert_eq!(
            store.load(&job.admission.job_id).err().unwrap().code,
            "job_store_recovery_required"
        );
        drop(store);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn interrupted_atomic_publication_is_preserved_and_refused() {
        let path = std::env::temp_dir().join(crate::result::new_id("job-partial-test"));
        let control = RunControl::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        let store = Store::open(&path, true, &control, deadline).unwrap();
        let job = store.create(admission()).unwrap();
        let before = std::fs::read(path.join(&job.admission.job_id).join("state.json")).unwrap();
        store::FAIL_BEFORE_PUBLISH.with(|flag| flag.set(true));
        let failure = store.commit(&job).unwrap_err();
        assert!(failure.effects);
        assert_eq!(failure.code, "job_test_write_failed");
        assert_eq!(
            std::fs::read(path.join(&job.admission.job_id).join("state.json")).unwrap(),
            before
        );
        drop(store);
        assert_eq!(
            Store::open(&path, false, &control, deadline)
                .err()
                .unwrap()
                .code,
            "job_store_recovery_required"
        );
        assert!(
            std::fs::read_dir(path.join(&job.admission.job_id))
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("pending-"))
        );
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn persisted_metadata_and_phase_corruption_are_rejected() {
        let path = std::env::temp_dir().join(crate::result::new_id("job-metadata-test"));
        let control = RunControl::default();
        let store = Store::open(
            &path,
            true,
            &control,
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
        let job = store.create(admission()).unwrap();
        let mut invalid = job.admission.clone();
        invalid.created_at = "invalid".into();
        std::fs::write(
            path.join(&job.admission.job_id).join("request.json"),
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap();
        assert_eq!(
            store.load(&job.admission.job_id).err().unwrap().code,
            "job_store_recovery_required"
        );
        for field in [
            "dispatched_at",
            "finished_at",
            "attempt_id",
            "result_sha256",
        ] {
            let mut value = serde_json::to_value(&job.state).unwrap();
            value[field] = json!("invalid");
            let state: State = serde_json::from_value(value).unwrap();
            assert!(!state.valid(&job.admission), "{field}");
        }
        let mut state = job.state.clone();
        state.events[0].at = "invalid".into();
        assert!(!state.valid(&job.admission));
        let mut state = job.state.clone();
        state.dispatched_at = Some(crate::result::timestamp());
        state.attempt_id = Some(crate::result::new_id("attempt"));
        assert!(!state.valid(&job.admission));
        let mut state = job.state.clone();
        state.events[0].state = "running".into();
        assert!(!state.valid(&job.admission));
        drop(store);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn queued_cancel_finalization_fault_preserves_effects_and_reconciles_without_dispatch() {
        let path = std::env::temp_dir().join(crate::result::new_id("job-finalizing-test"));
        let control = RunControl::default();
        let store = Store::open(
            &path,
            true,
            &control,
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
        let mut job = store.create(admission()).unwrap();
        job.state.cancel_requested = true;
        let child = dispatch::cancelled(&job.admission);
        api::FAIL_AFTER_FINALIZING.with(|flag| flag.set(true));
        let error = api::finalize(&store, &mut job, &child).unwrap_err();
        assert!(error.effects);
        assert_eq!(error.code, "job_deadline_exceeded");
        let loaded = store.load(&job.admission.job_id).unwrap();
        assert_eq!(loaded.state.state, "finalizing");
        assert!(loaded.state.dispatched_at.is_none());
        assert!(store.result(&loaded).unwrap().is_none());
        let id = job.admission.job_id.clone();
        drop(store);
        let context = OperatorContext {
            job_state_dir: Some(path.clone()),
            ..OperatorContext::default()
        };
        let mut request = Request::new("job.work", json!({"job_id":id}));
        request.record = "auto".into();
        let result = work(request, &control, &context);
        assert_eq!(
            result.execution.status,
            Status::Unknown,
            "{:?}",
            result.errors
        );
        assert_eq!(result.effect_outcome, "succeeded");
        assert_eq!(result.data["job"]["stored_state"], "recovery_required");
        assert!(result.data["result"].is_null());
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn terminal_state_summary_must_match_immutable_receipt() {
        let path = std::env::temp_dir().join(crate::result::new_id("job-summary-test"));
        let control = RunControl::default();
        let store = Store::open(
            &path,
            true,
            &control,
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
        let mut job = store.create(admission()).unwrap();
        job.state.state = "running".into();
        job.state.dispatched_at = Some(crate::result::timestamp());
        job.state.attempt_id = Some(crate::result::new_id("attempt"));
        job.state.effect_outcome = "unknown".into();
        job.state
            .event("running", None, job.admission.retain_events);
        store.commit(&job).unwrap();
        let mut child = OperationResult::new(&job.admission.request);
        child.run_id = job.admission.run_id.clone();
        child.execution.status = Status::Failed;
        child.effect_outcome = "unknown".into();
        api::finalize(&store, &mut job, &child).unwrap();
        assert!(store.result(&job).unwrap().is_some());
        job.state.execution_status = Some("succeeded".into());
        assert!(job.state.valid(&job.admission));
        assert_eq!(
            store.result(&job).err().unwrap().code,
            "job_store_recovery_required"
        );
        drop(store);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn late_failure_result_details_obey_budget_and_keep_digest() {
        let mut request = Request::new("job.work", json!({"job_id":"unused"}));
        request.limits.max_output_bytes = 1024;
        let mut result = OperationResult::new(&request);
        result.data = api::initial();
        let mut child = OperationResult::new(&Request::new("process.exec", json!({})));
        child.output.stdout = "x".repeat(4096);
        child.execution.child_exit_code = Some(0);
        api::failed_child_detail(&mut result, &child);
        assert_eq!(result.data["result_omitted"], true);
        assert!(result.data["result"].is_null());
        assert_eq!(
            result.data["result_sha256"],
            model::digest(&serde_json::to_vec(&child).unwrap())
        );
    }
    #[test]
    fn nested_null_control_fields_are_refused_before_store_creation() {
        let path = std::env::temp_dir().join(crate::result::new_id("job-null-test"));
        let context = OperatorContext {
            job_state_dir: Some(path.clone()),
            ..OperatorContext::default()
        };
        let original = serde_json::to_value(admission().request).unwrap();
        for (object, field) in [
            ("target", "environment"),
            ("limits", "max_items"),
            ("limits", "concurrency"),
        ] {
            let mut nested = original.clone();
            nested[object][field] = Value::Null;
            let mut request = Request::new(
                "job.submit",
                json!({"request":nested,"max_runtime_ms":1000,"retain_events":2}),
            );
            request.record = "auto".into();
            let result = crate::execute_with_context(request, &RunControl::default(), &context);
            assert_eq!(
                result.execution.status,
                Status::Denied,
                "{:?}",
                result.errors
            );
            assert!(!path.exists());
        }
    }
    #[test]
    fn write_receipts_preserve_outer_record_consent_and_child_never() {
        let path = std::env::temp_dir().join(crate::result::new_id("job-record-test"));
        let context = OperatorContext {
            job_state_dir: Some(path.clone()),
            ..OperatorContext::default()
        };
        let nested = Request::new(
            "process.exec",
            json!({"program":"/usr/bin/printf","args":["not dispatched"],"cwd":"/"}),
        );
        let mut submit = Request::new(
            "job.submit",
            json!({"request":nested,"max_runtime_ms":1000,"retain_events":2}),
        );
        submit.record = "always".into();
        let submitted = crate::execute_with_context(submit, &RunControl::default(), &context);
        assert_eq!(
            submitted.execution.status,
            Status::Succeeded,
            "{:?}",
            submitted.errors
        );
        assert_eq!(submitted.record_mode, "always");
        let id = submitted.data["job"]["job_id"].clone();
        let mut cancel = Request::new("job.cancel", json!({"job_id":id}));
        cancel.record = "always".into();
        let cancelled = crate::execute_with_context(cancel, &RunControl::default(), &context);
        assert_eq!(cancelled.record_mode, "always");
        assert_eq!(cancelled.data["result"]["record_mode"], "never");
        let mut worker = Request::new("job.work", json!({"job_id":id}));
        worker.record = "always".into();
        let worked = work(worker, &RunControl::default(), &context);
        assert_eq!(worked.record_mode, "always");
        assert_eq!(worked.data["result"]["record_mode"], "never");
        std::fs::remove_dir_all(path).unwrap();
    }
}
