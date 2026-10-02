#![cfg(all(feature = "test-fixtures", target_os = "linux"))]
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

const SAVE: &str = env!("CARGO_BIN_EXE_save");
const CASES: &str = include_str!("../../../tests/fixtures/error-budget/cases.json");

struct Fixture {
    directory: PathBuf,
    input: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "workbench-budget-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let input = directory.join("input.json");
        Self { directory, input }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(SAVE);
        command
            .args(["--json", "task", "run", "error-budget", "--input"])
            .arg(&self.input);
        command
    }
    fn run(&self, input: &Value) -> (Output, Value) {
        fs::write(&self.input, input.to_string()).unwrap();
        let output = self.command().output().unwrap();
        let value = receipt(&output);
        (output, value)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn receipt(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn error(result: &Value, code: &str) -> bool {
    result["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["code"] == code)
}
fn expect_subset(actual: &Value, expected: &Value, path: &str) {
    match expected {
        Value::Object(fields) => {
            for (key, value) in fields {
                expect_subset(&actual[key], value, &format!("{path}.{key}"));
            }
        }
        Value::Number(number) => {
            let expected = number.as_f64().unwrap();
            let actual = actual
                .as_f64()
                .unwrap_or_else(|| panic!("{path} is not numeric"));
            assert!(
                actual.is_finite()
                    && (actual - expected).abs() <= 1e-10_f64.max(expected.abs() * 1e-10),
                "{path}: {actual} != {expected}"
            );
            if expected == 0.0 && path.ends_with("remaining") {
                assert_eq!(actual, 0.0);
                assert!(!actual.is_sign_negative());
            }
        }
        other => assert_eq!(actual, other, "{path}"),
    }
}

#[test]
fn independent_numerical_goldens_match_without_turning_verdicts_into_process_failure() {
    let fixture = Fixture::new();
    let cases: Value = serde_json::from_str(CASES).unwrap();
    for case in cases.as_array().unwrap() {
        let (output, result) = fixture.run(&case["input"]);
        assert_eq!(output.status.code(), Some(0), "{}: {result}", case["name"]);
        assert_eq!(result["execution"]["status"], "succeeded");
        assert_eq!(result["assessment"], "not_assessed");
        assert_eq!(result["effect_outcome"], "not_applicable");
        expect_subset(
            &result["data"]["calculation"],
            &case["expected"],
            case["name"].as_str().unwrap(),
        );
        assert_eq!(result["output"]["stdout"], "");
        assert_eq!(
            fs::read_to_string(&fixture.input).unwrap(),
            case["input"].to_string()
        );
    }
}

#[test]
fn high_precision_finite_inputs_survive_the_real_child_protocol() {
    let fixture = Fixture::new();
    let input = json!({"slo":99.87654321098765,"window_days":1.2345678901234567e290,"bad_events":1.2345678901234566e290,"total_events":1.2345678901234567e290,"sli_long":98.98765432109876,"sli_short":97.87654321098765});
    let (output, result) = fixture.run(&input);
    assert_eq!(output.status.code(), Some(0), "{result}");
    assert_eq!(result["execution"]["status"], "succeeded");
    for (key, expected) in input.as_object().unwrap() {
        assert_eq!(&result["data"]["input"][key], expected, "{key}");
    }
}

#[test]
fn array_encodings_cannot_replace_object_inputs_or_envelopes() {
    let fixture = Fixture::new();
    for input in [
        json!([99.9]),
        json!([99.9, 28.0, null, null, null, null, null, "1h", "5m"]),
    ] {
        let (output, result) = fixture.run(&input);
        assert_eq!(output.status.code(), Some(2), "{result}");
        assert!(error(&result, "invalid_task_input"));
        assert_eq!(result["execution"]["child_exit_code"], Value::Null);
    }
    let request = json!({"spec_version":"0.1","request_id":"array-controls","operation":"task.run","operation_version":1,"target":["local","workstation",null],"inputs":{"id":"error-budget","version":1,"input":{"slo":99.9}},"limits":{"timeout_ms":30000,"max_output_bytes":1048576},"record":"never"});
    fs::write(&fixture.input, request.to_string()).unwrap();
    let output = Command::new(SAVE)
        .args(["--json", "call", "--request"])
        .arg(&fixture.input)
        .output()
        .unwrap();
    let result = receipt(&output);
    assert_eq!(output.status.code(), Some(2), "{result}");
    assert!(error(&result, "invalid_request"));
}

#[test]
fn typed_domain_and_exit_policy_failures_are_denied_before_dispatch() {
    let fixture = Fixture::new();
    for input in [
        json!({"slo":true}),
        json!({"slo":null}),
        json!({"slo":"99.9"}),
        json!({"slo":0}),
        json!({"slo":100}),
        json!({"slo":99.9,"window_days":0}),
        json!({"slo":99.9,"bad_minutes":-1}),
        json!({"slo":99.9,"bad_events":1}),
        json!({"slo":99.9,"total_events":1}),
        json!({"slo":99.9,"bad_events":2,"total_events":1}),
        json!({"slo":99.9,"bad_minutes":1,"bad_events":1,"total_events":2}),
        json!({"slo":99.9,"sli_short":99}),
        json!({"slo":99.9,"sli_long":101}),
        json!({"slo":99.9,"short_window":"30m"}),
        json!({"slo":99.9,"runtime":"override"}),
    ] {
        let (output, result) = fixture.run(&input);
        assert_eq!(output.status.code(), Some(2), "{input}: {result}");
        assert!(error(&result, "invalid_task_input"));
        assert_eq!(result["execution"]["child_exit_code"], Value::Null);
    }
    fixture.run(&json!({"slo":99.9}));
    let output = fixture
        .command()
        .arg("--fail-on-findings")
        .output()
        .unwrap();
    let result = receipt(&output);
    assert_eq!(output.status.code(), Some(2));
    assert!(error(&result, "unsupported_exit_policy"));
    assert_eq!(result["effect_outcome"], "not_attempted");
}

#[test]
fn nonfinite_or_duplicate_json_and_derived_range_errors_never_become_calculations() {
    let fixture = Fixture::new();
    for raw in [
        "{\"slo\":NaN}",
        "{\"slo\":Infinity}",
        "{\"slo\":1e999}",
        "{\"slo\":99,\"slo\":99.9}",
    ] {
        fs::write(&fixture.input, raw).unwrap();
        let output = fixture.command().output().unwrap();
        let result = receipt(&output);
        assert_eq!(output.status.code(), Some(2));
        assert!(error(&result, "invalid_json"));
    }
    for input in [
        json!({"slo":99.9,"window_days":1e308,"bad_minutes":0}),
        json!({"slo":99.9,"bad_minutes":1e308}),
        json!({"slo":99.9,"bad_events":0,"total_events":5e-324}),
    ] {
        let (output, result) = fixture.run(&input);
        assert_eq!(output.status.code(), Some(1), "{result}");
        assert!(error(&result, "numeric_out_of_range"));
        assert_eq!(result["execution"]["status"], "failed");
        assert!(result["data"].get("calculation").is_none());
    }
}

#[test]
fn text_call_and_cli_calculations_share_units_threshold_and_normalized_input() {
    let fixture = Fixture::new();
    let input =
        json!({"slo":99.9,"bad_events":50,"total_events":100000,"sli_long":98.5,"sli_short":98.5});
    let (_, cli) = fixture.run(&input);
    let output = Command::new(SAVE)
        .args(["task", "run", "error-budget", "--input"])
        .arg(&fixture.input)
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Budget: 100 events"));
    assert!(text.contains("Remaining: 50 events"));
    assert!(text.contains("short (5m): 15x"));
    assert!(text.contains("Threshold: 14.4x"));
    assert!(text.contains("Severity: page — both windows at or above threshold"));
    assert!(text.contains("not remaining-budget runway"));
    assert!(!text.contains("adapter_digest"));
    let request = json!({"spec_version":"0.1","request_id":"budget-parity","operation":"task.run","operation_version":1,"target":{"kind":"local","id":"workstation"},"inputs":{"id":"error-budget","version":1,"input":input},"limits":{"timeout_ms":30000,"max_output_bytes":1048576},"record":"never"});
    fs::write(&fixture.input, request.to_string()).unwrap();
    let output = Command::new(SAVE)
        .args(["--json", "call", "--request"])
        .arg(&fixture.input)
        .output()
        .unwrap();
    let call = receipt(&output);
    assert_eq!(call["data"]["calculation"], cli["data"]["calculation"]);
    assert_eq!(call["data"]["input"], cli["data"]["input"]);
}

#[test]
fn pure_python_regressions_run_in_the_isolated_supported_runtime() {
    let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../workbench-core/resources/error-budget/test_calculator.py");
    let output = Command::new("/usr/bin/python3")
        .args(["-I", "-S", "-B"])
        .arg(tests)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Ran 9 tests"));
}

#[test]
fn small_complete_reply_fits_the_minimum_capture_limit_but_deadline_still_applies() {
    let fixture = Fixture::new();
    fixture.run(
        &json!({"slo":99.9,"bad_events":50,"total_events":100000,"sli_long":98.5,"sli_short":98.5}),
    );
    let output = fixture
        .command()
        .args(["--timeout", "1ms"])
        .output()
        .unwrap();
    let result = receipt(&output);
    assert_eq!(result["execution"]["status"], "timed_out");
    assert!(result["data"].get("calculation").is_none());
    let output = fixture
        .command()
        .args(["--max-output-bytes", "1024"])
        .output()
        .unwrap();
    let result = receipt(&output);
    assert_eq!(output.status.code(), Some(0), "{result}");
    assert_eq!(result["execution"]["status"], "succeeded");
    assert!(!result["output"]["stdout_truncated"].as_bool().unwrap());
    assert!(!result["output"]["stderr_truncated"].as_bool().unwrap());
    assert!(
        result["data"]["supervisor"]["stdout_bytes_observed"]
            .as_u64()
            .unwrap()
            <= 1024
    );
    assert!(result["data"].get("calculation").is_some());
}
