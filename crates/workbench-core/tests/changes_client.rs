use serde_json::json;
use workbench_core::{RunControl, execute, request::Request};

#[test]
fn changes_require_authenticated_startup_authority() {
    let mut request = Request::new(
        "change.status",
        json!({"requester":"requester","request_id":"one"}),
    );
    request.target.kind = "runner".into();
    request.target.id = "fixture".into();
    let result = execute(request, &RunControl::default());
    assert_eq!(result.errors[0].code, "runner_config_required");
    assert_eq!(result.effect_outcome, "not_attempted");
}

#[test]
fn controlled_change_discovery_discloses_effects_and_configuration() {
    let value =
        workbench_core::discovery::capability("change.apply").expect("discovered operation");
    assert_eq!(value["effects"], "local_write");
    assert_eq!(
        workbench_core::discovery::availability("change.apply")["status"],
        "requires_configuration"
    );
}

#[test]
fn controlled_writes_require_recording_before_dispatch() {
    let mut request = Request::new("change.apply", json!({}));
    request.target.kind = "runner".into();
    request.target.id = "fixture".into();
    let result = execute(request, &RunControl::default());
    assert_eq!(result.errors[0].code, "recording_required");
    assert_eq!(result.effect_outcome, "not_attempted");
}
