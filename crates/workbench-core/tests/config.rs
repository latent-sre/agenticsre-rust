use serde_json::json;
use workbench_core::{
    OperatorContext, RunControl, execute_with_authority_identities, execute_with_context,
    request::Request, result::Status,
};
fn request() -> Request {
    Request::new(
        "config.check",
        json!({"kind":"grafana-dashboard","validator":{"id":"dashboard-hygiene","version":1},"file":"/definitely/absent/config.json"}),
    )
}
#[test]
fn hosted_config_checks_are_refused_before_any_model_access_or_child() {
    let result = execute_with_authority_identities(
        request(),
        &RunControl::default(),
        &OperatorContext::default(),
        None,
        None,
    );
    assert_eq!(result.execution.status, Status::Unsupported);
    assert_eq!(result.errors[0].code, "config_grants_unavailable");
    assert_eq!(result.effect_outcome, "not_attempted");
    assert!(result.data["supervisor"].is_null());
}
#[test]
fn validator_pin_refuses_before_missing_file_or_dependency_and_preserves_identity() {
    let mut request = request();
    request.inputs["validator"]["expected_binding_sha256"] =
        json!(format!("sha256:{}", "0".repeat(64)));
    let result = execute_with_context(request, &RunControl::default(), &OperatorContext::default());
    assert_eq!(result.execution.status, Status::Denied);
    assert_eq!(result.errors[0].code, "validator_digest_mismatch");
    assert_eq!(result.effect_outcome, "not_attempted");
    assert!(result.data["input"]["digest"].is_null());
    assert!(result.data["input"]["bytes"].is_null());
    assert!(result.data["supervisor"].is_null());
    assert_eq!(
        result.data["implemented_rules"].as_array().unwrap().len(),
        10
    );
    assert!(result.text().contains("Validator:"));
    assert!(result.text().contains("Validation: not_established"));
}
#[test]
fn config_checks_refuse_recording_unknown_versions_and_null_pins() {
    let mut recorded = request();
    recorded.record = "always".into();
    assert_eq!(
        recorded.validate().unwrap_err().code,
        "unsupported_record_mode"
    );
    for (field, value) in [
        ("validator", json!(["dashboard-hygiene", 1])),
        ("expected_input_sha256", serde_json::Value::Null),
        ("file", json!("relative.json")),
    ] {
        let mut invalid = request();
        invalid.inputs[field] = value;
        let result =
            execute_with_context(invalid, &RunControl::default(), &OperatorContext::default());
        assert_eq!(result.execution.status, Status::Denied);
        assert_eq!(result.effect_outcome, "not_attempted");
        assert!(result.data["supervisor"].is_null());
    }
}

fn rules_request() -> Request {
    Request::new(
        "config.check",
        json!({"kind":"prometheus-rules","validator":{"id":"promtool","version":1},"file":"/definitely/absent/rules.yml"}),
    )
}
#[test]
fn prometheus_pins_and_hosted_refusals_precede_executable_or_model_access() {
    let mut pinned = rules_request();
    pinned.inputs["validator"]["expected_binding_sha256"] =
        json!(format!("sha256:{}", "0".repeat(64)));
    let context = OperatorContext {
        promtool_path: Some("/definitely/absent/promtool".into()),
        ..Default::default()
    };
    let result = execute_with_context(pinned, &RunControl::default(), &context);
    assert_eq!(result.errors[0].code, "validator_digest_mismatch");
    assert_eq!(result.execution.status, Status::Denied);
    assert_eq!(result.effect_outcome, "not_attempted");
    assert!(result.data["input"]["digest"].is_null());
    let result = execute_with_authority_identities(
        rules_request(),
        &RunControl::default(),
        &context,
        None,
        None,
    );
    assert_eq!(result.errors[0].code, "config_grants_unavailable");
    assert_eq!(result.effect_outcome, "not_attempted");
    let mut invalid = rules_request();
    invalid.inputs["validator"]["path"] = json!("/tmp/arbitrary");
    assert_eq!(
        execute_with_context(invalid, &RunControl::default(), &context).errors[0].code,
        "invalid_config_input"
    );
    let mut wrong_kind = rules_request();
    wrong_kind.inputs["validator"]["id"] = json!("dashboard-hygiene");
    assert_eq!(
        execute_with_context(wrong_kind, &RunControl::default(), &context).errors[0].code,
        "unsupported_validator"
    );
}
#[cfg(all(
    target_os = "linux",
    target_arch = "x86_64",
    target_pointer_width = "64"
))]
#[test]
fn missing_or_unreviewed_promtool_never_reads_the_rule_model() {
    let missing = execute_with_context(
        rules_request(),
        &RunControl::default(),
        &OperatorContext::default(),
    );
    assert_eq!(missing.errors[0].code, "promtool_required");
    assert!(missing.data["input"]["digest"].is_null());
    let path = std::env::temp_dir().join(workbench_core::result::new_id("unreviewed-promtool"));
    std::fs::write(&path, b"not the reviewed executable").unwrap();
    let context = OperatorContext {
        promtool_path: Some(path.clone()),
        ..Default::default()
    };
    let result = execute_with_context(rules_request(), &RunControl::default(), &context);
    assert_eq!(result.errors[0].code, "unsupported_promtool_binary");
    assert_eq!(result.execution.status, Status::Denied);
    assert!(result.data["input"]["digest"].is_null());
    assert!(result.data["supervisor"].is_null());
    assert!(result.data["runtime"].is_null());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"not the reviewed executable"
    );
    std::fs::remove_file(path).unwrap();
}
