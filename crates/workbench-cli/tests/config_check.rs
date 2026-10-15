#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::Command};

const SAVE: &str = env!("CARGO_BIN_EXE_save");
const CLEAN: &[u8] = include_bytes!("../../../tests/fixtures/dashboard-hygiene/clean.json");
const FINDINGS: &[u8] = include_bytes!("../../../tests/fixtures/dashboard-hygiene/findings.json");

fn digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}
struct Fixture {
    directory: PathBuf,
    model: PathBuf,
}
impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        let directory = std::env::temp_dir().join(workbench_core::result::new_id("config-test"));
        fs::create_dir(&directory).unwrap();
        let model = directory.join("model.json");
        fs::write(&model, bytes).unwrap();
        Self { directory, model }
    }
    fn input(&self) -> Value {
        json!({"kind":"grafana-dashboard","validator":{"id":"dashboard-hygiene","version":1},"file":self.model})
    }
    fn invoke(&self, input: &Value, extra: &[&str], exit: i32) -> Value {
        let path = self.directory.join("input.json");
        fs::write(&path, input.to_string()).unwrap();
        let output = Command::new(SAVE)
            .env_clear()
            .args(["--json", "config", "check", "--input"])
            .arg(path)
            .args(extra)
            .output()
            .unwrap();
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(exit), "{result}");
        assert_eq!(result["assessment"], "not_assessed");
        result
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn declared_check_binds_exact_bytes_validator_rules_and_unsupported_scope() {
    let f = Fixture::new(CLEAN);
    let mut input = f.input();
    input["expected_input_sha256"] = json!(digest(CLEAN));
    let r = f.invoke(&input, &[], 0);
    assert_eq!(r["operation"], "config.check");
    assert_eq!(r["execution"]["status"], "succeeded");
    assert_eq!(r["data"]["validation_state"], "complete");
    assert_eq!(r["data"]["input"]["digest"], digest(CLEAN));
    assert_eq!(r["data"]["input"]["bytes"], CLEAN.len());
    assert_eq!(r["data"]["findings_total"], 0);
    assert_eq!(r["data"]["implemented_rules"].as_array().unwrap().len(), 10);
    assert!(
        r["data"]["unsupported_sections"]
            .as_array()
            .unwrap()
            .contains(&json!("live_datasource_queries"))
    );
    input["validator"]["expected_binding_sha256"] =
        r["data"]["validator"]["binding_digest"].clone();
    let pinned = f.invoke(&input, &[], 0);
    assert_eq!(r["data"]["validator"], pinned["data"]["validator"]);
    assert_eq!(fs::read(&f.model).unwrap(), CLEAN);
}

#[test]
fn findings_change_optional_cli_exit_without_health_or_execution_claims() {
    let f = Fixture::new(FINDINGS);
    let ordinary = f.invoke(&f.input(), &[], 0);
    let policy = f.invoke(&f.input(), &["--fail-on-findings"], 1);
    assert!(ordinary["data"]["findings_total"].as_u64().unwrap() > 0);
    assert_eq!(ordinary["data"]["findings"], policy["data"]["findings"]);
    assert_eq!(policy["execution"]["status"], "succeeded");
    assert_eq!(policy["data"]["validation_state"], "complete");
    assert_eq!(fs::read(&f.model).unwrap(), FINDINGS);
}

#[test]
fn only_declared_supported_envelopes_can_reach_complete_validation() {
    let f = Fixture::new(CLEAN);
    let clean: Value = serde_json::from_slice(CLEAN).unwrap();
    for model in [
        json!({"dashboard":clean,"meta":{}}),
        json!({"kind":"Dashboard","apiVersion":"dashboard.grafana.app/v1beta1","spec":clean}),
    ] {
        fs::write(&f.model, model.to_string()).unwrap();
        let r = f.invoke(&f.input(), &[], 0);
        assert_eq!(r["data"]["validation_state"], "complete");
    }
    for model in [
        json!({"kind":"Dashboard","apiVersion":"dashboard.grafana.app/v99","spec":clean}),
        json!({"kind":"AlertRule","apiVersion":"dashboard.grafana.app/v1beta1","spec":clean}),
        json!({"kind":"Dashboard","apiVersion":"dashboard.grafana.app/v1beta1","spec":clean,"dashboard":clean,"meta":{}}),
    ] {
        let bytes = model.to_string();
        fs::write(&f.model, &bytes).unwrap();
        let r = f.invoke(&f.input(), &[], 1);
        assert_eq!(r["data"]["validation_state"], "unsupported");
        assert_eq!(r["data"]["checked_panels"], 0);
        assert_eq!(r["data"]["input"]["digest"], digest(bytes.as_bytes()));
    }
}

#[test]
fn invalid_and_unsupported_models_retain_actual_input_identity_without_clean_result() {
    for (bytes, code) in [
        (
            b"{\"title\":\"sensitive-canary\",".as_slice(),
            "invalid_model_json",
        ),
        (
            b"{\"panels\":[],\"panels\":[]}".as_slice(),
            "duplicate_model_key",
        ),
        (b"{\"panels\":[]}".as_slice(), "uncheckable_model"),
        (
            b"{\"elements\":{},\"layout\":{}}".as_slice(),
            "unsupported_model_v2",
        ),
    ] {
        let f = Fixture::new(bytes);
        let r = f.invoke(&f.input(), &[], 1);
        assert!(
            r["errors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["code"] == code),
            "{r}"
        );
        assert_ne!(r["data"]["validation_state"], "complete");
        assert_eq!(r["data"]["input"]["digest"], digest(bytes));
        assert_eq!(r["data"]["input"]["bytes"], bytes.len());
        assert!(!r.to_string().contains("sensitive-canary"));
        assert_eq!(fs::read(&f.model).unwrap(), bytes);
    }
}

#[test]
fn mismatched_pins_are_explicit_and_validator_pin_precedes_file_access() {
    let f = Fixture::new(CLEAN);
    let mut input = f.input();
    input["expected_input_sha256"] = json!(digest(b"different"));
    let r = f.invoke(&input, &[], 1);
    assert!(
        r["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["code"] == "input_digest_mismatch")
    );
    assert_eq!(r["data"]["input"]["digest"], digest(CLEAN));
    assert_eq!(r["data"]["checked_panels"], 0);
    input = f.input();
    input["validator"]["expected_binding_sha256"] = json!(digest(b"wrong-binding"));
    input["file"] = json!(f.directory.join("missing.json"));
    let r = f.invoke(&input, &[], 2);
    assert!(
        r["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["code"] == "validator_digest_mismatch")
    );
    assert_eq!(r["effect_outcome"], "not_attempted");
    assert!(r["data"]["supervisor"].is_null());
    assert_eq!(fs::read(&f.model).unwrap(), CLEAN);
}

#[test]
fn malformed_contract_and_partial_findings_cannot_pass_by_omission() {
    let f = Fixture::new(CLEAN);
    for (key, value) in [
        ("kind", json!("unknown")),
        ("validator", json!(["dashboard-hygiene", 1])),
        ("file", json!("relative.json")),
        ("file", json!("/tmp/control\ncharacter.json")),
        ("expected_input_sha256", Value::Null),
        ("command", json!("echo override")),
    ] {
        let mut input = f.input();
        input[key] = value;
        let r = f.invoke(&input, &[], 2);
        assert_eq!(r["effect_outcome"], "not_attempted");
        assert!(r["data"]["supervisor"].is_null());
    }
    let mut model: Value = serde_json::from_slice(CLEAN).unwrap();
    model["panels"][0]["title"] = json!("long".repeat(200));
    fs::write(&f.model, model.to_string()).unwrap();
    let r = f.invoke(&f.input(), &[], 1);
    assert_eq!(r["execution"]["status"], "partial");
    assert_eq!(r["data"]["validation_state"], "partial");
    assert_eq!(r["data"]["fields_truncated"], true);
}
