#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

const SAVE: &str = env!("CARGO_BIN_EXE_save");
const OP: &str = "ext.example.capacity.project";
fn digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}
fn inputs(capacity: u64, used: u64, growth: u64) -> Value {
    json!({"capacity_bytes":capacity,"used_bytes":used,"growth_bytes_per_day":growth})
}
struct Fixture {
    root: PathBuf,
    store: PathBuf,
    policy: PathBuf,
    pin: String,
}
impl Fixture {
    fn empty() -> Self {
        let root = std::env::temp_dir().join(workbench_core::result::new_id("extension-runtime"));
        fs::create_dir(&root).unwrap();
        Self {
            store: root.join("store"),
            policy: root.join("policy.json"),
            root,
            pin: String::new(),
        }
    }
    fn installed(variable: &str) -> Self {
        let mut f = Self::empty();
        let binary = std::env::var_os(variable)
            .unwrap_or_else(|| panic!("Set {variable} to the explicitly built native fixture"));
        let executable = fs::read(binary).unwrap();
        let mut manifest: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/extensions/manifest.json"
        ))
        .unwrap();
        manifest["id"] = json!("example.capacity");
        manifest["executable"]["sha256"] = json!(digest(&executable));
        manifest["executable"]["bytes"] = json!(executable.len());
        manifest["capabilities"][0]["operation"] = json!(OP);
        manifest["capabilities"][0]["output_schema"] = json!({
            "$schema":"https://json-schema.org/draft/2020-12/schema", "type":"object", "additionalProperties":false,
            "properties":{
                "headroom_bytes":{"type":"integer","minimum":0,"maximum":9007199254740991u64},
                "days_remaining_floor":{"type":"integer","minimum":0,"maximum":9007199254740991u64},
                "exhausted":{"type":"boolean"}, "has_finite_horizon":{"type":"boolean"},
                "projection_state":{"type":"string","minLength":1,"maxLength":32},
                "assumption":{"type":"string","minLength":1,"maxLength":128}
            },
            "required":["headroom_bytes","days_remaining_floor","exhausted","has_finite_horizon","projection_state","assumption"]
        });
        let package = serde_json::to_vec(&json!({"package_format":1,"manifest":manifest,"executable_base64":STANDARD.encode(executable)})).unwrap();
        f.pin = digest(&package);
        let path = f.root.join("package.json");
        fs::write(&path, package).unwrap();
        let installed = f.run(
            false,
            &[
                "extension",
                "install",
                path.to_str().unwrap(),
                "--expected-package-sha256",
                &f.pin,
            ],
            0,
        );
        assert_eq!(installed["data"]["enabled"], false);
        f.write_policy(&f.pin);
        f
    }
    fn write_policy(&self, pin: &str) {
        fs::write(&self.policy, serde_json::to_vec(&json!({"policy_version":1,"execution_profile":"operator-reviewed-v1","packages":[{"package_sha256":pin,"capabilities":[OP]}]})).unwrap()).unwrap();
        fs::set_permissions(&self.policy, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn run(&self, authorized: bool, args: &[&str], exit: i32) -> Value {
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .args(["--json", "--extension-state-dir"])
            .arg(&self.store);
        if authorized {
            command.arg("--extension-policy").arg(&self.policy);
        }
        let output = command.args(args).output().unwrap();
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.len() <= 2 * 1024 * 1024);
        let result: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stdout)));
        assert_eq!(output.status.code(), Some(exit), "{result}");
        assert_eq!(result["assessment"], "not_assessed");
        result
    }
    fn project(&self, authorized: bool, input: Value, exit: i32) -> Value {
        let path = self.root.join("input.json");
        fs::write(&path, input.to_string()).unwrap();
        self.run(
            authorized,
            &["extension", "run", OP, "--input", path.to_str().unwrap()],
            exit,
        )
    }
    fn call(&self, authorized: bool, id: &str, version: u64, exit: i32) -> Value {
        let mut request = workbench_core::request::Request::new(OP, inputs(1000, 500, 100));
        request.request_id = id.into();
        request.operation_version = version;
        let path = self.root.join("request.json");
        fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
        self.run(
            authorized,
            &["call", "--request", path.to_str().unwrap()],
            exit,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn has_error(result: &Value, code: &str) {
    assert!(
        result["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == code),
        "{result}"
    );
}
fn no_child(result: &Value) {
    assert!(result["execution"]["child_exit_code"].is_null(), "{result}");
    assert!(result["data"]["supervisor"].is_null(), "{result}");
    assert_eq!(result["effect_outcome"], "not_attempted");
}

#[test]
fn direct_external_request_requires_independent_policy_before_store_or_child() {
    let f = Fixture::empty();
    let result = f.call(false, "unconfigured-extension", 1, 2);
    assert_eq!(result["execution"]["status"], "denied");
    has_error(&result, "extension_policy_required");
    no_child(&result);
    assert!(!f.store.exists());
}

#[test]
#[ignore = "Requires WORKBENCH_TEST_CAPACITY_PEER naming the explicitly built native executable"]
fn native_capacity_calculations_and_discovery_require_exact_package_grants() {
    let f = Fixture::installed("WORKBENCH_TEST_CAPACITY_PEER");
    let installed = f.run(false, &["extension", "list"], 0);
    assert_eq!(installed["data"]["execution_available"], false);
    let granted = f.run(true, &["extension", "list"], 0);
    assert_eq!(granted["data"]["execution_available"], true);
    let disabled_description = f.run(false, &["capabilities", "describe", OP], 0);
    assert_eq!(
        disabled_description["data"]["availability"]["status"],
        "requires_configuration"
    );
    let enabled_description = f.run(true, &["capabilities", "describe", OP], 0);
    assert_eq!(
        enabled_description["data"]["availability"]["status"],
        "available"
    );
    assert_eq!(enabled_description["data"]["capability"]["id"], OP);
    for description in [&disabled_description, &enabled_description] {
        assert!(description["execution"]["child_exit_code"].is_null());
        assert!(description["data"]["supervisor"].is_null());
        assert_eq!(description["effect_outcome"], "not_applicable");
    }
    for (input, headroom, days, exhausted, finite, state) in [
        (inputs(1000, 500, 100), 500, 5, false, true, "growing"),
        (inputs(1000, 500, 0), 500, 0, false, false, "not_growing"),
        (inputs(1000, 1001, 0), 0, 0, true, true, "already_full"),
        (inputs(1000, 999, 2), 1, 0, false, true, "growing"),
        (
            inputs(9007199254740991, 0, 1),
            9007199254740991u64,
            9007199254740991u64,
            false,
            true,
            "growing",
        ),
    ] {
        let result = f.project(true, input, 0);
        assert_eq!(result["execution"]["status"], "succeeded");
        assert_eq!(result["execution"]["child_exit_code"], 0);
        assert_eq!(result["effect_outcome"], "unknown");
        assert_eq!(result["data"]["package"]["package_sha256"], f.pin);
        assert_eq!(result["data"]["protocol"]["terminal_received"], true);
        assert_eq!(result["data"]["terminal"]["status"], "succeeded");
        let data = &result["data"]["terminal"]["data"];
        assert_eq!(data["headroom_bytes"], headroom);
        assert_eq!(data["days_remaining_floor"], days);
        assert_eq!(data["exhausted"], exhausted);
        assert_eq!(data["has_finite_horizon"], finite);
        assert_eq!(data["projection_state"], state);
        assert_eq!(
            data["assumption"],
            "Constant supplied daily growth; no live measurements."
        );
        assert_eq!(result["data"]["supervisor"]["cleanup"]["confirmed"], true);
    }
}

#[test]
#[ignore = "Requires WORKBENCH_TEST_CAPACITY_PEER naming the explicitly built native executable"]
fn native_admission_refuses_missing_grants_bad_pins_inputs_versions_and_policy_permissions() {
    let f = Fixture::installed("WORKBENCH_TEST_CAPACITY_PEER");
    let result = f.project(false, inputs(1000, 500, 100), 2);
    assert_eq!(result["execution"]["status"], "denied");
    has_error(&result, "extension_policy_required");
    no_child(&result);
    f.write_policy(&digest(b"different package"));
    let result = f.project(true, inputs(1000, 500, 100), 2);
    assert_eq!(result["execution"]["status"], "denied");
    has_error(&result, "extension_not_authorized");
    no_child(&result);
    f.write_policy(&f.pin);
    let mut input = inputs(1000, 500, 100);
    input["argv"] = json!(["unrequested"]);
    let result = f.project(true, input, 2);
    assert_eq!(result["execution"]["status"], "denied");
    has_error(&result, "invalid_extension_arguments");
    no_child(&result);
    let result = f.call(true, "old-extension-consumer", 2, 2);
    assert_eq!(result["execution"]["status"], "unsupported");
    has_error(&result, "unsupported_version");
    no_child(&result);
    fs::set_permissions(&f.policy, fs::Permissions::from_mode(0o644)).unwrap();
    let result = f.project(true, inputs(1000, 500, 100), 2);
    assert_eq!(result["execution"]["status"], "denied");
    has_error(&result, "extension_policy_invalid");
    no_child(&result);
}

#[test]
#[ignore = "Requires WORKBENCH_TEST_EXTENSION_ADVERSARY naming the explicitly built test-fixtures executable"]
fn native_bad_peers_cannot_turn_protocol_or_process_failure_into_success() {
    let f = Fixture::installed("WORKBENCH_TEST_EXTENSION_ADVERSARY");
    for mode in [
        "unsupported",
        "correlation",
        "sequence",
        "duplicate-terminal",
        "missing-terminal",
    ] {
        let result = f.call(true, &format!("fixture-{mode}"), 1, 1);
        assert_eq!(result["execution"]["status"], "failed", "{mode}: {result}");
        assert_eq!(result["effect_outcome"], "unknown");
        has_error(&result, "extension_protocol_error");
        assert!(result["data"]["supervisor"]["process_id"].is_number());
        assert_eq!(result["data"]["supervisor"]["cleanup"]["confirmed"], true);
        if mode == "missing-terminal" {
            assert_eq!(result["execution"]["child_exit_code"], 0);
        }
        if mode == "duplicate-terminal" {
            assert_eq!(result["data"]["terminal"]["status"], "succeeded");
        }
    }
    let nonzero = f.call(true, "fixture-nonzero", 1, 1);
    assert_eq!(nonzero["execution"]["status"], "failed");
    assert_eq!(nonzero["execution"]["child_exit_code"], 7);
    assert_eq!(nonzero["data"]["terminal"]["status"], "succeeded");
    assert_eq!(nonzero["effect_outcome"], "unknown");
    let failed = f.call(true, "fixture-failed", 1, 1);
    assert_eq!(failed["execution"]["status"], "failed");
    assert_eq!(failed["execution"]["child_exit_code"], 0);
    assert_eq!(
        failed["data"]["terminal"]["error"]["code"],
        "fixture_failure"
    );
    has_error(&failed, "extension_reported_failure");
}
