use serde_json::json;
use workbench_core::{
    EvidenceStoreOptions, OperatorContext, RunControl, execute_with_authority_identities,
    execute_with_context,
    request::Request,
    result::{Status, new_id},
};

#[test]
fn hosted_evidence_cannot_inherit_operator_store_authority() {
    let directory = std::env::temp_dir().join(new_id("absent-evidence"));
    let context = OperatorContext {
        evidence_store: Some(EvidenceStoreOptions::new(directory.clone())),
        ..Default::default()
    };
    for operation in [
        "evidence.packs.list",
        "evidence.packs.describe",
        "evidence.collect",
        "evidence.compare",
        "evidence.open",
        "evidence.replay",
        "evidence.list",
        "evidence.show",
        "evidence.protect",
        "evidence.unprotect",
        "evidence.delete",
        "evidence.prune",
        "evidence.recover",
    ] {
        let mut request = Request::new(operation, json!({}));
        if operation == "evidence.collect" {
            request.record = "always".into();
            request.target.kind = "connection".into();
            request.target.id = "fixture".into();
        }
        let result = execute_with_authority_identities(
            request,
            &RunControl::default(),
            &context,
            None,
            None,
        );
        assert_eq!(result.execution.status, Status::Unsupported, "{operation}");
        assert_eq!(
            result.errors[0].code, "evidence_grants_unavailable",
            "{operation}"
        );
        assert_eq!(result.effect_outcome, "not_attempted");
        assert!(!directory.exists());
    }
}

#[test]
fn offline_discovery_and_malformed_references_never_initialize_store() {
    let directory = std::env::temp_dir().join(new_id("absent-evidence"));
    let context = OperatorContext {
        evidence_store: Some(EvidenceStoreOptions::new(directory.clone())),
        ..Default::default()
    };
    let result = execute_with_context(
        Request::new("evidence.packs.list", json!({})),
        &RunControl::default(),
        &context,
    );
    assert_eq!(result.execution.status, Status::Succeeded);
    for operation in [
        "evidence.show",
        "evidence.open",
        "evidence.replay",
        "evidence.protect",
        "evidence.delete",
    ] {
        let result = execute_with_context(
            Request::new(operation, json!({"id":"../../unowned"})),
            &RunControl::default(),
            &context,
        );
        assert_eq!(result.execution.status, Status::Denied);
        assert_eq!(result.errors[0].code, "invalid_evidence_reference");
    }
    let result = execute_with_context(
        Request::new("evidence.list", json!({})),
        &RunControl::default(),
        &context,
    );
    assert_ne!(result.execution.status, Status::Succeeded);
    assert!(!directory.exists());
}

#[test]
fn collection_does_not_silently_record_under_never() {
    let mut request = Request::new("evidence.collect", json!({}));
    request.target.kind = "connection".into();
    request.target.id = "fixture".into();
    let result = execute_with_context(request, &RunControl::default(), &OperatorContext::default());
    assert_eq!(result.execution.status, Status::Denied);
    assert_eq!(result.errors[0].code, "recording_required");
    assert_eq!(result.effect_outcome, "not_attempted");
    assert_eq!(result.record_mode, "never");
}

#[test]
fn comparison_discovery_is_observational_and_offline() {
    let result = execute_with_context(
        Request::new(
            "capability.describe",
            json!({"operation":"evidence.compare"}),
        ),
        &RunControl::default(),
        &OperatorContext::default(),
    );
    assert_eq!(result.execution.status, Status::Succeeded);
    assert_eq!(result.data["capability"]["effects"], "observe");
    assert_eq!(result.data["capability"]["required_permissions"], json!([]));
}

#[test]
fn offline_operations_refuse_paths_plugins_recording_and_source_access() {
    let directory = std::env::temp_dir().join(new_id("absent-offline"));
    let context = OperatorContext {
        evidence_store: Some(EvidenceStoreOptions::new(directory.clone())),
        config_path: Some(directory.join("invalid-config")),
        context_source_path: Some(directory.join("invalid-context")),
        ..Default::default()
    };
    for operation in ["evidence.open", "evidence.replay"] {
        let discovery = workbench_core::discovery::capability(operation).unwrap();
        assert_eq!(discovery["effects"], "observe");
        assert_eq!(discovery["required_permissions"], json!([]));
        for input in [
            json!({"id":"/tmp/evidence"}),
            json!({"id":"run-1-2-3","command":"echo injected"}),
            json!({"id":"run-1-2-3","algorithm":"plugin"}),
            json!({"id":"run-1-2-3","expected_manifest_sha256":null}),
        ] {
            let result = execute_with_context(
                Request::new(operation, input),
                &RunControl::default(),
                &context,
            );
            assert_eq!(result.errors[0].code, "invalid_evidence_reference");
        }
        let mut request = Request::new(operation, json!({"id":"run-1-2-3"}));
        request.record = "always".into();
        assert_eq!(
            execute_with_context(request, &RunControl::default(), &context).errors[0].code,
            "unsupported_record_mode"
        );
        let mut request = Request::new(operation, json!({"id":"run-1-2-3"}));
        request.limits.max_items = Some(1001);
        assert!(request.validate().is_err());
        request.limits.max_items = Some(1);
        assert!(request.validate().is_ok());
        let control = RunControl::default();
        control
            .signal
            .store(2, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            execute_with_context(request, &control, &context)
                .execution
                .status,
            Status::Cancelled
        );
        assert!(!directory.exists());
    }
}
