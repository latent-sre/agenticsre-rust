use serde_json::json;
use workbench_core::{
    EvidenceStoreOptions, OperatorContext, RunControl, execute_with_authority_identities,
    execute_with_context,
    request::Request,
    result::{Status, new_id},
};

#[test]
fn reports_are_observational_and_hosted_grants_do_not_inherit_store_authority() {
    let directory = std::env::temp_dir().join(new_id("absent-report"));
    let context = OperatorContext {
        evidence_store: Some(EvidenceStoreOptions::new(directory.clone())),
        ..Default::default()
    };
    let request = Request::new(
        "report.create",
        json!({"bundles":[{"id":"run-1-2-3"}],"format":"json"}),
    );
    let result =
        execute_with_authority_identities(request, &RunControl::default(), &context, None, None);
    assert_eq!(result.execution.status, Status::Unsupported);
    assert_eq!(result.errors[0].code, "report_grants_unavailable");
    assert!(!directory.exists());
    let capability = workbench_core::discovery::capability("report.create").unwrap();
    assert_eq!(capability["effects"], "observe");
    assert_eq!(capability["required_permissions"], json!([]));
}

#[test]
fn invalid_report_input_never_opens_store_or_uses_source_configuration() {
    let directory = std::env::temp_dir().join(new_id("absent-report"));
    let context = OperatorContext {
        evidence_store: Some(EvidenceStoreOptions::new(directory.clone())),
        config_path: Some(directory.join("bad-config")),
        context_source_path: Some(directory.join("bad-source")),
        ..Default::default()
    };
    for input in [
        json!({"bundles":[{"id":"/tmp/source"}],"format":"json"}),
        json!({"bundles":[{"id":"run-1-2-3"}],"format":"json","publish":true}),
        json!({"bundles":[{"id":"run-1-2-3"}],"format":"json","proposals":[{"kind":"runbook","title":"x","content":"healthy","evidence":["run-1-2-4"]}]}),
    ] {
        let result = execute_with_context(
            Request::new("report.create", input),
            &RunControl::default(),
            &context,
        );
        assert_eq!(result.execution.status, Status::Denied);
        assert_eq!(result.errors[0].code, "invalid_report_input");
        assert!(!directory.exists());
    }
    let mut request = Request::new(
        "report.create",
        json!({"bundles":[{"id":"run-1-2-3"}],"format":"json"}),
    );
    request.record = "always".into();
    assert_eq!(
        execute_with_context(request, &RunControl::default(), &context).errors[0].code,
        "unsupported_record_mode"
    );
}
