#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::Command};

const SAVE: &str = env!("CARGO_BIN_EXE_save");
const CLEAN: &[u8] = include_bytes!("../../../tests/fixtures/prometheus-rules/clean.yml");
fn digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}
struct Fixture {
    directory: PathBuf,
    model: PathBuf,
}
impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        let directory = std::env::temp_dir().join(workbench_core::result::new_id("rules-test"));
        fs::create_dir(&directory).unwrap();
        let model = directory.join("rules.yml");
        fs::write(&model, bytes).unwrap();
        Self { directory, model }
    }
    fn input(&self) -> Value {
        json!({"kind":"prometheus-rules","validator":{"id":"promtool","version":1},"file":self.model})
    }
    fn run(&self, input: &Value, promtool: Option<&str>, extra: &[&str], exit: i32) -> Value {
        let typed = self.directory.join("input.json");
        fs::write(&typed, input.to_string()).unwrap();
        let mut command = Command::new(SAVE);
        command.env_clear().arg("--json");
        if let Some(path) = promtool {
            command.args(["--promtool", path]);
        }
        let output = command
            .args(["config", "check", "--input"])
            .arg(typed)
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
fn binary() -> String {
    std::env::var("WORKBENCH_TEST_PROMTOOL")
        .expect("Set WORKBENCH_TEST_PROMTOOL to the verified official executable")
}

#[test]
fn missing_dependency_and_unpinned_binary_refuse_before_input_or_child() {
    let f = Fixture::new(CLEAN);
    let mut input = f.input();
    input["file"] = json!(f.directory.join("missing.yml"));
    let missing = f.run(&input, None, &[], 2);
    assert!(
        missing["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["code"] == "promtool_required")
    );
    assert_eq!(missing["effect_outcome"], "not_attempted");
    let wrong_binary = fs::canonicalize("/usr/bin/true").unwrap();
    let wrong = f.run(&input, wrong_binary.to_str(), &[], 2);
    assert!(
        wrong["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["code"] == "unsupported_promtool_binary")
    );
    assert!(wrong["data"]["input"]["digest"].is_null());
    assert!(wrong["data"]["supervisor"].is_null());
}

#[test]
fn wire_input_cannot_select_executable_flags_or_bypass_kind_binding() {
    let f = Fixture::new(CLEAN);
    for (key, value) in [
        ("executable", json!("/usr/bin/true")),
        ("args", json!(["check", "healthy"])),
        ("validator", json!({"id":"dashboard-hygiene","version":1})),
        ("expected_input_sha256", Value::Null),
    ] {
        let mut input = f.input();
        input[key] = value;
        let r = f.run(&input, None, &[], 2);
        assert_eq!(r["effect_outcome"], "not_attempted");
        assert!(r["data"]["supervisor"].is_null());
    }
}

#[test]
#[ignore = "Requires the SHA-256-verified official promtool fixture via WORKBENCH_TEST_PROMTOOL"]
fn native_rule_and_expression_validation_preserves_consumed_identity_and_original() {
    let binary = binary();
    let f = Fixture::new(CLEAN);
    let mut input = f.input();
    input["expected_input_sha256"] = json!(digest(CLEAN));
    let r = f.run(&input, Some(&binary), &[], 0);
    assert_eq!(r["data"]["validation_outcome"], "valid");
    assert_eq!(r["data"]["checked_rules"], 2);
    assert_eq!(r["data"]["input"]["digest"], digest(CLEAN));
    assert_eq!(r["data"]["runtime"]["version"], "3.15.0");
    assert_eq!(fs::read(&f.model).unwrap(), CLEAN);
    for bytes in [
        include_bytes!("../../../tests/fixtures/prometheus-rules/invalid-promql.yml").as_slice(),
        include_bytes!("../../../tests/fixtures/prometheus-rules/invalid-template.yml").as_slice(),
        include_bytes!("../../../tests/fixtures/prometheus-rules/unknown-field.yml").as_slice(),
    ] {
        fs::write(&f.model, bytes).unwrap();
        let r = f.run(&f.input(), Some(&binary), &[], 1);
        assert_eq!(r["data"]["validation_outcome"], "invalid");
        assert_eq!(r["execution"]["child_exit_code"], 1);
        assert_eq!(r["data"]["input"]["digest"], digest(bytes));
        assert_eq!(fs::read(&f.model).unwrap(), bytes);
    }
}

#[test]
#[ignore = "Requires the SHA-256-verified official promtool fixture via WORKBENCH_TEST_PROMTOOL"]
fn native_lint_empty_and_unchecked_second_document_never_become_clean_success() {
    let binary = binary();
    let bytes = include_bytes!("../../../tests/fixtures/prometheus-rules/duplicate.yml");
    let f = Fixture::new(bytes);
    let r = f.run(&f.input(), Some(&binary), &[], 0);
    assert_eq!(r["data"]["validation_outcome"], "lint_findings");
    assert_eq!(r["execution"]["child_exit_code"], 3);
    assert!(r["data"]["findings_total"].as_u64().unwrap() > 0);
    f.run(&f.input(), Some(&binary), &["--fail-on-findings"], 1);
    fs::write(&f.model, b"groups: []\n").unwrap();
    let r = f.run(&f.input(), Some(&binary), &[], 1);
    assert_eq!(r["data"]["validation_outcome"], "uncheckable");
    let multi = include_bytes!("../../../tests/fixtures/prometheus-rules/multiple-documents.yml");
    fs::write(&f.model, multi).unwrap();
    let r = f.run(&f.input(), Some(&binary), &[], 1);
    assert_eq!(r["execution"]["child_exit_code"], 0);
    assert_eq!(r["execution"]["status"], "partial");
    assert_eq!(r["data"]["validation_outcome"], "unknown");
    assert!(
        r["data"]["diagnostics"]["stderr"]
            .as_str()
            .unwrap()
            .contains("Multiple document")
    );
}

#[test]
#[ignore = "Requires the SHA-256-verified official promtool fixture via WORKBENCH_TEST_PROMTOOL"]
fn native_pins_and_output_bounds_preserve_honest_refusals() {
    let binary = binary();
    let f = Fixture::new(CLEAN);
    let mut input = f.input();
    input["expected_input_sha256"] = json!(digest(b"not this input"));
    let r = f.run(&input, Some(&binary), &[], 1);
    assert_eq!(r["data"]["input"]["digest"], digest(CLEAN));
    assert!(r["data"]["supervisor"].is_null());
    assert_eq!(r["effect_outcome"], "not_attempted");
    let rules = (0..100)
        .map(|i| format!("      - alert: Broken{i}\n        expr: sum(\n"))
        .collect::<String>();
    fs::write(
        &f.model,
        format!("groups:\n  - name: many\n    rules:\n{rules}"),
    )
    .unwrap();
    let r = f.run(
        &f.input(),
        Some(&binary),
        &["--max-output-bytes", "1024"],
        1,
    );
    assert_eq!(r["data"]["validation_state"], "partial");
    assert_eq!(r["data"]["diagnostics"]["truncated"], true);
}
