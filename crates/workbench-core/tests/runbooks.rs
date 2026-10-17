use serde_json::json;
use workbench_core::{
    OperatorContext, RunControl, execute_with_authority_identities, execute_with_context,
    request::Request, result::Status,
};

#[test]
fn hosted_runbooks_cannot_inherit_journal_authority() {
    let directory = std::env::temp_dir().join(workbench_core::result::new_id("absent-runbook"));
    let context = OperatorContext {
        runbook_state_dir: Some(directory.clone()),
        ..Default::default()
    };
    for operation in [
        "runbook.list",
        "runbook.show",
        "runbook.status",
        "runbook.run",
        "runbook.resume",
    ] {
        let mut request = Request::new(operation, json!({}));
        if matches!(operation, "runbook.run" | "runbook.resume") {
            request.record = "always".into();
        }
        let result = execute_with_authority_identities(
            request,
            &RunControl::default(),
            &context,
            None,
            None,
        );
        assert_eq!(result.execution.status, Status::Unsupported);
        assert_eq!(result.errors[0].code, "runbook_grants_unavailable");
        assert_eq!(result.effect_outcome, "not_attempted");
        assert!(!directory.exists());
    }
}

#[test]
fn durable_runbooks_require_recording_and_publish_truthful_effects() {
    for operation in ["runbook.run", "runbook.resume"] {
        let result = execute_with_context(
            Request::new(operation, json!({})),
            &RunControl::default(),
            &OperatorContext::default(),
        );
        assert_eq!(result.execution.status, Status::Denied);
        assert_eq!(result.errors[0].code, "recording_required");
        assert_eq!(
            workbench_core::discovery::capability(operation).unwrap()["effects"],
            "local_write"
        );
    }
    for operation in ["runbook.list", "runbook.show", "runbook.status"] {
        assert_eq!(
            workbench_core::discovery::capability(operation).unwrap()["effects"],
            "observe"
        );
    }
}
