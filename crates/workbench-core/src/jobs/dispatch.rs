use super::*;
use model::Admission;
pub(super) fn admit(
    request: &mut Request,
    control: &RunControl,
    context: &OperatorContext,
    deadline: Instant,
) -> Result<Value, Failure> {
    request.validate().map_err(|p| Failure::new(p.code))?;
    active(control, deadline)?;
    if !supported(&request.operation) || request.record != "never" {
        return Err(Failure::new("unsupported_job_operation"));
    }
    if context.read_policy_path.is_some()
        && matches!(request.operation.as_str(), "process.exec" | "task.run")
    {
        return Err(Failure::new("job_read_policy_unsupported"));
    }
    let identity = match request.operation.as_str() {
        "process.exec" => {
            let prepared =
                crate::process::prepare(&request.inputs).map_err(|p| Failure::new(p.code))?;
            request.inputs = json!({"program":prepared.executable,"args":prepared.input.args,"cwd":prepared.cwd});
            json!({"kind":"process","implementation_sha256":model::digest(&[include_bytes!("../process.rs").as_slice(),include_bytes!("../process/linux.rs").as_slice(),include_bytes!("../process/descriptor.rs").as_slice()].concat()),"program":request.inputs["program"],"cwd":request.inputs["cwd"],"version":env!("CARGO_PKG_VERSION")})
        }
        "task.run" => {
            crate::tasks::validate_installed_input(&request.inputs)
                .map_err(|p| Failure::new(p.code))?;
            let mut result = OperationResult::new(request);
            crate::tasks::describe(&json!({"id":request.inputs["id"]}), &mut result);
            json!({"kind":"task","id":request.inputs["id"],"version":request.inputs["version"],"source":result.data["source"]})
        }
        _ => crate::extensions::job_binding(request, control, context, deadline)
            .map_err(|p| Failure::new(p.code))?,
    };
    active(control, deadline)?;
    Ok(identity)
}
pub(super) fn execute(
    admission: &Admission,
    control: &RunControl,
    context: &OperatorContext,
) -> OperationResult {
    if admission.implementation["kind"] == "remote" {
        return super::remote::execute(admission, control, context, None);
    }
    let start = Instant::now();
    let mut result = OperationResult::new(&admission.request);
    result.run_id = admission.run_id.clone();
    // Validate the unchanged ordinary envelope before privately applying the distinct job budget.
    if let Err(problem) = admission.request.validate() {
        result.reject(problem);
        result.finish(start);
        return result;
    }
    let mut request = admission.request.clone();
    request.limits.timeout_ms = admission.max_runtime_ms;
    result.effective_limits.timeout_ms = admission.max_runtime_ms;
    result.effective_limits.max_output_bytes =
        result.effective_limits.max_output_bytes.min(256 * 1024);
    request.limits.max_output_bytes = result.effective_limits.max_output_bytes;
    let deadline = start + Duration::from_millis(admission.max_runtime_ms);
    if context.read_policy_path.is_some()
        && matches!(request.operation.as_str(), "process.exec" | "task.run")
    {
        result.reject(Problem::invalid(
            "job_binding_changed",
            "A command read policy cannot be ignored by the local job worker.",
        ));
    } else if request.operation.starts_with("ext.") {
        crate::extensions::execute_job(
            &request,
            control,
            context,
            start,
            &mut result,
            &admission.implementation,
        );
        if result.effect_outcome == "not_attempted"
            && matches!(
                result.execution.status,
                Status::Denied | Status::Unsupported | Status::Failed
            )
            && !result
                .errors
                .iter()
                .any(|e| e.code == "job_binding_changed")
        {
            result.execution.status = Status::Denied;
            result.error("job_binding_changed","The exact current package and explicit policy could not satisfy the immutable job binding; no extension child started.");
        }
    } else {
        let mut ordinary = admission.request.clone();
        match admit(&mut ordinary, control, context, deadline) {
            Ok(binding) if binding == admission.implementation => {
                match request.operation.as_str() {
                    "process.exec" => match crate::process::prepare(&request.inputs) {
                        Ok(prepared) => {
                            result.resolved_target = prepared.identity();
                            crate::process::run(prepared, &mut result, control)
                        }
                        Err(p) => result.reject(p),
                    },
                    "task.run" => crate::tasks::run(&request, control, &mut result),
                    _ => result.reject(Problem::unsupported(
                        "unsupported_job_operation",
                        "This operation is not admitted for local jobs.",
                    )),
                }
            }
            Err(error)
                if matches!(
                    error.code,
                    "job_deadline_exceeded" | "job_cancelled" | "job_consumer_closed"
                ) =>
            {
                fail(error, control, &mut result);
            }
            _ => result.reject(Problem::invalid(
                "job_binding_changed",
                "The admitted implementation or current authority changed before dispatch.",
            )),
        }
    }
    result.finish(start);
    bound(&mut result);
    result
}
pub(super) fn cancelled(admission: &Admission) -> OperationResult {
    let mut result = OperationResult::new(&admission.request);
    result.run_id = admission.run_id.clone();
    result.execution.status = Status::Cancelled;
    result.effective_limits.timeout_ms = admission.max_runtime_ms;
    result.error(
        "job_cancelled_before_dispatch",
        "Cancellation was committed before child dispatch; no operation ran.",
    );
    result
}
fn bound(result: &mut OperationResult) {
    while serde_json::to_vec(result).expect("result").len() > model::MAX_RESULT - 4096 {
        let output = if result.output.stdout.len() >= result.output.stderr.len() {
            result.output.stdout_truncated = true;
            &mut result.output.stdout
        } else {
            result.output.stderr_truncated = true;
            &mut result.output.stderr
        };
        if output.is_empty() {
            // Supported adapters already bound structured data to 256KiB. Preserve a
            // failure receipt instead of storing an oversized apparently complete result.
            result.data = json!({"job_structured_output_omitted":true});
            result.error(
                "job_result_truncated",
                "Structured child detail exceeded the durable result bound and was omitted.",
            );
        } else {
            let mut length = output.len() / 2;
            while !output.is_char_boundary(length) {
                length -= 1;
            }
            output.truncate(length);
        }
        if result.execution.status == Status::Succeeded {
            result.execution.status = Status::Partial;
        }
        result.coverage.state = "partial".into();
    }
    result.bound_output();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_preflight_preserves_deadline_and_cancellation_without_starting_child() {
        let context = OperatorContext::default();
        let control = RunControl::default();
        let mut request = Request::new(
            "process.exec",
            json!({"program":"/usr/bin/printf","args":["must not run"],"cwd":"/"}),
        );
        let implementation = admit(
            &mut request,
            &control,
            &context,
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
        let mut admission = Admission {
            format_version: 1,
            job_id: crate::result::new_id("job"),
            run_id: crate::result::new_id("run"),
            owner_uid: uid(),
            request_sha256: model::fingerprint(&request, 1, 2, &implementation, uid()),
            request,
            // Internal boundary fixture: make the deadline already elapsed without
            // depending on host load or changing public admission's 1 ms minimum.
            max_runtime_ms: 0,
            retain_events: 2,
            created_at: crate::result::timestamp(),
            implementation,
        };
        let expired = execute(&admission, &control, &context);
        assert_eq!(expired.execution.status, Status::TimedOut);
        assert_eq!(expired.errors[0].code, "job_deadline_exceeded");
        assert_eq!(expired.effect_outcome, "not_attempted");
        assert!(expired.output.stdout.is_empty());
        assert!(expired.execution.child_exit_code.is_none());

        admission.max_runtime_ms = 1000;
        control.signal.store(15, Ordering::Relaxed);
        let cancelled = execute(&admission, &control, &context);
        assert_eq!(cancelled.execution.status, Status::Cancelled);
        assert_eq!(cancelled.errors[0].code, "job_cancelled");
        assert_eq!(cancelled.data["cancellation_signal"], 15);
        assert_eq!(cancelled.effect_outcome, "not_attempted");
        assert!(cancelled.output.stdout.is_empty());
        assert!(cancelled.execution.child_exit_code.is_none());

        control.signal.store(0, Ordering::Relaxed);
        admission.implementation = json!({"kind":"changed"});
        let changed = execute(&admission, &control, &context);
        assert_eq!(changed.execution.status, Status::Denied);
        assert_eq!(changed.errors[0].code, "job_binding_changed");
        assert_eq!(changed.effect_outcome, "not_attempted");
    }
}
