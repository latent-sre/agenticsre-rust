use serde_json::json;
use workbench_core::{
    OperatorContext, RunControl, execute_with_policy_identity, read_policy_description,
    request::{Request, parse_bounded_json},
    tasks::normalize_error_budget_input,
};

#[test]
fn browser_json_depth_and_duplicates_are_checked_before_typed_inputs() {
    assert_eq!(
        parse_bounded_json(br#"{"inputs":{"slo":99,"slo":98}}"#, 24)
            .unwrap_err()
            .code,
        "invalid_json"
    );
    let deep = format!("{}0{}", "[".repeat(25), "]".repeat(25));
    assert_eq!(
        parse_bounded_json(deep.as_bytes(), 24).unwrap_err().code,
        "request_too_deep"
    );
    assert!(parse_bounded_json(br#"{"slo":99.9}"#, 24).is_ok());
}

#[test]
fn numerical_schema_normalizes_defaults_and_refuses_file_input() {
    let normalized = normalize_error_budget_input(&json!({"slo":99.9})).unwrap();
    assert_eq!(
        normalized,
        json!({"slo":99.9,"window_days":28.0,"long_window":"1h","short_window":"5m"})
    );
    for value in [
        json!({"file":"/tmp/canary"}),
        json!([99.9]),
        json!({"slo":99.9,"file":"/tmp/canary"}),
        json!({"slo":true}),
        json!({"slo":99.9,"bad_minutes":null}),
        json!({"slo":99.9,"bad_events":2}),
    ] {
        assert_eq!(
            normalize_error_budget_input(&value).unwrap_err().code,
            "invalid_task_input"
        );
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn replaced_policy_is_refused_at_actual_core_load_before_bindings() {
    use std::os::unix::fs::PermissionsExt;
    let directory = std::env::temp_dir().join(workbench_core::result::new_id("gui-policy"));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("policy.json");
    let binding = json!({"path":"/usr/bin/absent-gui-test-binding","sha256":format!("sha256:{}","0".repeat(64))});
    let mut policy = json!({"version":1,"profile":"linux-read-v1","roots":[{"id":"checkout","path":"/tmp/gui-old-root"}],"executables":{"git":binding,"rg":binding,"bwrap":binding}});
    std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let description = read_policy_description(&path).unwrap();
    assert_eq!(description.roots[0].path, "/tmp/gui-old-root");
    policy["roots"][0]["path"] = json!("/tmp/gui-replaced-root");
    std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
    let request = Request::new(
        "command.inspect",
        json!({"program":"rg","args":["--files"],"cwd":"/tmp/gui-old-root"}),
    );
    let result = execute_with_policy_identity(
        request,
        &RunControl::default(),
        &OperatorContext {
            read_policy_path: Some(path),
            ..Default::default()
        },
        Some(&description.digest),
    );
    assert_eq!(
        result.execution.status,
        workbench_core::result::Status::Denied
    );
    assert_eq!(result.errors[0].code, "read_policy_changed");
    assert_eq!(result.effect_outcome, "not_attempted");
    assert!(result.data.get("process_id").is_none());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn pinned_policy_requires_restricted_context_and_never_falls_back() {
    let result = execute_with_policy_identity(
        Request::new(
            "command.inspect",
            json!({"program":"/usr/bin/printf","args":["no fallback"],"cwd":"/"}),
        ),
        &RunControl::default(),
        &OperatorContext::default(),
        Some("sha256:pinned"),
    );
    assert_eq!(result.errors[0].code, "read_policy_required");
    assert_eq!(result.effect_outcome, "not_attempted");
}
