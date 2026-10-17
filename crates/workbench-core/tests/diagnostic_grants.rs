#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use workbench_core::{OperatorContext, RunControl, request::Request};

struct Fixture {
    path: PathBuf,
    bytes: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(workbench_core::result::new_id("diagnostic-grant"));
        let bytes = serde_json::to_vec(&json!({"schema_version":1,"targets":[{
            "id":"fixture-web","host":"127.0.0.1","port":9,"path":"/probe",
            "allowed_ips":["127.0.0.1"],"resolver":{"kind":"system","transport":"udp"}},
            {"id":"hidden","host":"127.0.0.1","port":9,"path":"/private",
            "allowed_ips":["127.0.0.1"],"resolver":{"kind":"system","transport":"udp"}}]}))
        .unwrap();
        fs::write(&path, &bytes).unwrap();
        Self { path, bytes }
    }
    fn context(&self) -> OperatorContext {
        OperatorContext {
            diagnostics_config_path: Some(self.path.clone()),
            ..OperatorContext::default()
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_file(&self.path).unwrap();
    }
}
fn request(target: &str) -> Request {
    let mut request = Request::new(
        "diagnostics.run",
        json!({"id":"connectivity","version":1,"input":{}}),
    );
    request.target.kind = "connection".into();
    request.target.id = target.into();
    request.limits.timeout_ms = 1000;
    request
}

#[test]
fn generic_hosted_context_never_infers_diagnostic_grants_from_operator_configuration() {
    let fixture = Fixture::new();
    let receipt = workbench_core::execute_with_authority_identities(
        request("fixture-web"),
        &RunControl::default(),
        &fixture.context(),
        None,
        None,
    );
    assert_eq!(receipt.errors[0].code, "diagnostics_grants_unavailable");
    assert_eq!(receipt.effect_outcome, "not_attempted");
    assert_eq!(receipt.data, json!({}));
}

#[test]
fn diagnostic_grant_requires_unique_known_explicit_aliases_and_admitted_files() {
    let fixture = Fixture::new();
    for ids in [
        vec![],
        vec!["missing".into()],
        vec!["fixture-web".into(), "fixture-web".into()],
    ] {
        assert!(workbench_core::diagnostics_grant(&fixture.path, &ids).is_err());
    }
    let grant = workbench_core::diagnostics_grant(&fixture.path, &["fixture-web".into()]).unwrap();
    assert_eq!(grant.target_ids().collect::<Vec<_>>(), vec!["fixture-web"]);
    let receipt = workbench_core::execute_with_diagnostic_grant(
        request("hidden"),
        &RunControl::default(),
        &fixture.context(),
        None,
        &grant,
    );
    assert_eq!(receipt.errors[0].code, "diagnostics_target_not_granted");
    assert_eq!(receipt.effect_outcome, "not_attempted");
    assert_eq!(receipt.data, json!({}));
}

#[test]
fn changed_configuration_bytes_refuse_before_observation_but_identical_replacement_preserves_grant()
{
    let fixture = Fixture::new();
    let grant = workbench_core::diagnostics_grant(&fixture.path, &["fixture-web".into()]).unwrap();
    fs::write(&fixture.path, [fixture.bytes.as_slice(), b" "].concat()).unwrap();
    let receipt = workbench_core::execute_with_diagnostic_grant(
        request("fixture-web"),
        &RunControl::default(),
        &fixture.context(),
        None,
        &grant,
    );
    assert_eq!(receipt.errors[0].code, "diagnostics_identity_changed");
    assert_eq!(receipt.effect_outcome, "not_attempted");
    assert_eq!(receipt.data, json!({}));
    let replacement = fixture.path.with_extension("replacement");
    fs::write(&replacement, &fixture.bytes).unwrap();
    fs::rename(replacement, &fixture.path).unwrap();
    let receipt = workbench_core::execute_with_diagnostic_grant(
        request("fixture-web"),
        &RunControl::default(),
        &fixture.context(),
        None,
        &grant,
    );
    assert_eq!(receipt.effect_outcome, "not_applicable");
    assert_eq!(receipt.data["binding"]["target"], "fixture-web");
    assert_eq!(
        receipt.data["steps"][0]["observation"]["mode"],
        "literal_ip"
    );
}

#[test]
fn invalid_regular_file_replacement_refuses_instead_of_using_cached_config_bytes() {
    let fixture = Fixture::new();
    let grant = workbench_core::diagnostics_grant(&fixture.path, &["fixture-web".into()]).unwrap();
    fs::remove_file(&fixture.path).unwrap();
    std::os::unix::fs::symlink("/dev/null", &fixture.path).unwrap();
    let receipt = workbench_core::execute_with_diagnostic_grant(
        request("fixture-web"),
        &RunControl::default(),
        &fixture.context(),
        None,
        &grant,
    );
    assert_eq!(receipt.errors[0].code, "diagnostics_file_read_failed");
    assert_eq!(receipt.effect_outcome, "not_attempted");
    assert_eq!(receipt.data, Value::Object(Default::default()));
}
