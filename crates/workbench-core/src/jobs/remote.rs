//! Private runner seam: principal namespaces are authenticated before these calls.
use super::*;
use model::Admission;
use store::Store;
pub(crate) fn lookup(
    path: &std::path::Path,
    request_id: &str,
    principal: &str,
    runner: &str,
    c: &RunControl,
    d: Instant,
) -> Result<Option<(String, Value)>, &'static str> {
    let store = Store::open(path, false, c, d).map_err(|e| e.code)?;
    for id in store.ids().map_err(|e| e.code)? {
        let job = store.load(&id).map_err(|e| e.code)?;
        if job.admission.implementation["kind"] != "remote"
            || job.admission.implementation["scope"]["principal"] != principal
            || job.admission.implementation["scope"]["runner_id"] != runner
        {
            return Err("runner_namespace_mismatch");
        }
        if job.admission.request.request_id == request_id {
            return Ok(Some((id, job.admission.implementation.clone())));
        }
    }
    Ok(None)
}
pub(crate) fn initialize(
    path: &std::path::Path,
    c: &RunControl,
    d: Instant,
) -> Result<(), &'static str> {
    Store::open(path, true, c, d)
        .map(|_| ())
        .map_err(|e| e.code)
}
pub(crate) fn pending(
    path: &std::path::Path,
    principal: &str,
    runner: &str,
    c: &RunControl,
    d: Instant,
) -> Result<Vec<(String, Value)>, &'static str> {
    let store = Store::open(path, false, c, d).map_err(|e| e.code)?;
    let mut rows = vec![];
    for id in store.ids().map_err(|e| e.code)? {
        let job = store.load(&id).map_err(|e| e.code)?;
        if job.admission.implementation["scope"]["principal"] != principal
            || job.admission.implementation["scope"]["runner_id"] != runner
        {
            return Err("runner_namespace_mismatch");
        }
        if !matches!(job.state.state.as_str(), "terminal" | "recovery_required") {
            rows.push((id, job.admission.implementation));
        }
    }
    Ok(rows)
}
pub(crate) fn submit(
    input: &Value,
    scope: Value,
    context: &OperatorContext,
    c: &RunControl,
    d: Instant,
) -> OperationResult {
    let mut request = Request::new("job.submit", json!({}));
    request.record = "always".into();
    let mut result = OperationResult::new(&request);
    result.record_mode = "always";
    result.data = api::initial();
    let outcome = (|| {
        let path = api::path(context)?;
        let store = Store::open(path, false, c, d)?;
        let submission = model::digest(&serde_json::to_vec(input).unwrap());
        for id in store.ids()? {
            let job = store.load(&id)?;
            if Some(job.admission.request.request_id.as_str()) == input["request_id"].as_str() {
                if job.admission.implementation["kind"] != "remote"
                    || job.admission.implementation["scope"]["principal"] != scope["principal"]
                    || job.admission.implementation["scope"]["runner_id"] != scope["runner_id"]
                    || job.admission.implementation["submission_sha256"] != submission
                {
                    return Err(Failure::new("job_request_conflict"));
                }
                api::respond(&request, &store, &job, "existing", &mut result)?;
                result.data["remote_binding"] = job.admission.implementation.clone();
                return Ok(());
            }
        }
        let mut nested = Request::new(
            input["operation"].as_str().unwrap(),
            input["inputs"].clone(),
        );
        nested.request_id = input["request_id"].as_str().unwrap().into();
        nested.limits.max_output_bytes = 256 * 1024;
        let implementation = dispatch::admit(&mut nested, c, context, d)?;
        crate::runner::check_grant(&nested, &implementation, &scope).map_err(Failure::new)?;
        let expires_at = (time::OffsetDateTime::now_utc() + time::Duration::seconds(60))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        let binding = json!({"kind":"remote","scope":scope,"expires_at":expires_at,"submission_sha256":submission,"implementation":implementation});
        let runtime = input["max_runtime_ms"].as_u64().unwrap();
        let retention = input["retain_events"].as_u64().unwrap() as usize;
        let fingerprint = model::fingerprint(&nested, runtime, retention, &binding, uid());
        let job = store.create(Admission {
            format_version: 1,
            job_id: crate::result::new_id("job"),
            run_id: crate::result::new_id("run"),
            owner_uid: uid(),
            request: nested,
            request_sha256: fingerprint,
            max_runtime_ms: runtime,
            retain_events: retention,
            created_at: crate::result::timestamp(),
            implementation: binding.clone(),
        })?;
        result.effect_outcome = "succeeded".into();
        api::respond(&request, &store, &job, "submitted", &mut result)?;
        result.data["remote_binding"] = binding;
        Ok(())
    })();
    if let Err(e) = outcome {
        fail(e, c, &mut result);
    }
    result
}
pub(crate) fn observe(
    method: &str,
    principal: &str,
    runner: &str,
    input: &Value,
    context: &OperatorContext,
    c: &RunControl,
    d: Instant,
) -> OperationResult {
    let mut request = Request::new(&format!("job.{method}"), json!({}));
    if method == "cancel" {
        request.record = "always".into();
    }
    let mut result = OperationResult::new(&request);
    result.record_mode = if method == "cancel" {
        "always"
    } else {
        "never"
    };
    result.data = api::initial();
    let found = api::path(context).and_then(|p| {
        lookup(
            p,
            input["request_id"].as_str().unwrap(),
            principal,
            runner,
            c,
            d,
        )
        .map_err(Failure::new)
    });
    match found {
        Ok(Some((id, binding))) => {
            request.inputs = if method == "watch" {
                json!({"job_id":id,"after_sequence":input["after_sequence"]})
            } else {
                json!({"job_id":id})
            };
            let remaining = d.saturating_duration_since(Instant::now()).as_millis() as u64;
            if remaining == 0 {
                fail(Failure::new("job_deadline_exceeded"), c, &mut result);
                return result;
            }
            request.limits.timeout_ms = remaining.min(300_000);
            api::run(&request, c, context, Instant::now(), &mut result);
            result.data["remote_binding"] = binding;
        }
        Ok(None) => {
            result.data["not_found"] = json!(true);
            result.effect_outcome = "not_applicable".into();
        }
        Err(e) => fail(e, c, &mut result),
    }
    result
}
pub(crate) fn work(
    id: &str,
    scope: &Value,
    context: &OperatorContext,
    c: &RunControl,
) -> OperationResult {
    let mut req = Request::new("job.work", json!({"job_id":id}));
    req.record = "always".into();
    let mut result = OperationResult::new(&req);
    worker::run_scoped(&req, c, context, Instant::now(), &mut result, Some(scope));
    result
}
pub(super) fn execute(
    admission: &Admission,
    c: &RunControl,
    context: &OperatorContext,
    scope: Option<&Value>,
) -> OperationResult {
    let binding = &admission.implementation;
    let allowed = scope.is_some_and(|s| s == &binding["scope"])
        && time::OffsetDateTime::parse(
            binding["expires_at"].as_str().unwrap_or(""),
            &time::format_description::well_known::Rfc3339,
        )
        .is_ok_and(|t| time::OffsetDateTime::now_utc() < t);
    if !allowed {
        let mut result = OperationResult::new(&admission.request);
        result.run_id = admission.run_id.clone();
        result.execution.status = Status::Denied;
        result.error("runner_binding_changed_or_expired","The authenticated authority, target snapshot, or original dispatch admission expiry is no longer valid; no child started.");
        return result;
    }
    let mut admitted = admission.clone();
    admitted.implementation = binding["implementation"].clone();
    if crate::runner::check_grant(&admitted.request, &admitted.implementation, scope.unwrap())
        .is_err()
    {
        let mut result = OperationResult::new(&admitted.request);
        result.run_id = admitted.run_id;
        result.execution.status = Status::Denied;
        result.error(
            "runner_grant_denied",
            "The exact frozen target grant does not permit this installed implementation.",
        );
        return result;
    }
    dispatch::execute(&admitted, c, context)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn durable_remote_dedup_preserves_expiry_and_isolated_recovery_handles() {
        let path = std::env::temp_dir().join(crate::result::new_id("runner-jobs-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(10);
        initialize(&path, &c, d).unwrap();
        let context = OperatorContext {
            job_state_dir: Some(path.clone()),
            ..OperatorContext::default()
        };
        let mut request = Request::new(
            "task.run",
            json!({"id":"error-budget","version":1,"input":{"slo":99.9,"bad_minutes":1}}),
        );
        let implementation = dispatch::admit(&mut request, &c, &context, d).unwrap();
        let scope = json!({"principal":"alice","runner_id":"fixture","grant":{"operation":"task.run","operation_version":1,"task":{"id":"error-budget","version":1,"binding_sha256":implementation["source"]["binding_digest"]}}});
        let input = json!({"request_id":"known-before-ack","target":"fixture","operation":"task.run","operation_version":1,"inputs":request.inputs,"max_runtime_ms":1000,"retain_events":2});
        let first = submit(&input, scope.clone(), &context, &c, d);
        assert_eq!(
            first.execution.status,
            Status::Succeeded,
            "{:?}",
            first.errors
        );
        let second = submit(&input, scope.clone(), &context, &c, d);
        assert_eq!(second.data["action"], "existing");
        assert_eq!(first.data["job"], second.data["job"]);
        assert_eq!(
            first.data["remote_binding"]["expires_at"],
            second.data["remote_binding"]["expires_at"]
        );
        assert!(
            lookup(&path, "known-before-ack", "alice", "fixture", &c, d)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            lookup(&path, "known-before-ack", "bob", "fixture", &c, d).unwrap_err(),
            "runner_namespace_mismatch"
        );
        let mut changed = input.clone();
        changed["max_runtime_ms"] = json!(2000);
        let conflict = submit(&changed, scope.clone(), &context, &c, d);
        assert!(
            conflict
                .errors
                .iter()
                .any(|e| e.code == "job_request_conflict")
        );
        let store = Store::open(&path, false, &c, d).unwrap();
        let id = first.data["job"]["job_id"].as_str().unwrap();
        let mut admission = store.load(id).unwrap().admission;
        drop(store);
        let denied = dispatch::execute(&admission, &c, &context);
        assert_eq!(denied.execution.status, Status::Denied);
        assert_eq!(denied.execution.child_exit_code, None);
        assert_eq!(denied.effect_outcome, "not_attempted");
        admission.implementation["expires_at"] = json!("2000-01-01T00:00:00Z");
        let expired = execute(&admission, &c, &context, Some(&scope));
        assert_eq!(expired.execution.status, Status::Denied);
        assert_eq!(expired.execution.child_exit_code, None);
        let cancelled = observe(
            "cancel",
            "alice",
            "fixture",
            &json!({"request_id":"known-before-ack"}),
            &context,
            &c,
            d,
        );
        assert_eq!(cancelled.data["job"]["state"], "terminal");
        assert_eq!(cancelled.data["result"]["execution"]["status"], "cancelled");
        std::fs::remove_dir_all(path).unwrap();
    }
}
