use serde_json::json;
use workbench_core::{
    OperatorContext, RunControl, execute_with_authority_identities, grafana_config_description,
    request::{MAX_RESULT_BYTES, Request},
    result::{OperationResult, Status, new_id},
};

fn request() -> Request {
    let mut request = Request::new(
        "query.run",
        json!({"id":"prometheus.up-by-job","version":1,"datasource":"metrics-1",
            "parameters":{"job":"private-fixture"},
            "window":{"kind":"relative","lookback_seconds":300}}),
    );
    request.target.kind = "connection".into();
    request.target.id = "fixture".into();
    request
}

#[test]
fn hosted_saved_queries_cannot_inherit_an_existing_grafana_grant() {
    let config = std::env::temp_dir().join(new_id("query-grant"));
    std::fs::write(
        &config,
        json!({"spec_version":"0.1","record":"never","connections":{"fixture":{
            "origin":"https://127.0.0.1:9","expected_org_id":7,
            "api_profile":"grafana-legacy-v1","credential_ref":"env:SAVE_GRAFANA_UNUSED_FIXTURE"}}})
        .to_string(),
    )
    .unwrap();
    let (_, identity) = grafana_config_description(&config).unwrap();
    for identity in [None, Some(&identity)] {
        let result = execute_with_authority_identities(
            request(),
            &RunControl::default(),
            &OperatorContext {
                config_path: Some(config.clone()),
                ..Default::default()
            },
            None,
            identity,
        );
        assert_eq!(result.execution.status, Status::Unsupported);
        assert_eq!(result.errors[0].code, "query_grants_unavailable");
        assert_eq!(result.effect_outcome, "not_attempted");
        assert_eq!(result.data, json!({}));
        assert!(
            !String::from_utf8(result.json_bytes())
                .unwrap()
                .contains("private-fixture")
        );
    }
    std::fs::remove_file(config).unwrap();
}

#[test]
fn finalized_query_receipt_omits_oversized_response_and_preserves_definition() {
    let mut result = OperationResult::new(&request());
    let identity = json!({"id":"prometheus.up-by-job","version":1,
        "dialect":"promql","sha256":"a".repeat(64)});
    result.data = json!({"definition":identity,"response":{"large":"x".repeat(MAX_RESULT_BYTES)},
        "cancellation_signal":null});
    result.finish(std::time::Instant::now());
    assert!(result.json_bytes().len() <= MAX_RESULT_BYTES);
    assert_eq!(result.execution.status, Status::Failed);
    assert_eq!(result.data["definition"], identity);
    assert!(result.data["response"].is_null());
    assert_eq!(result.coverage.state, "not_established");
    assert_eq!(result.errors[0].code, "structured_response_too_large");
}
