#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

const FIXTURE: &str = include_str!("../../../tests/fixtures/context/unique.json");
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

fn fresh_fixture() -> Value {
    let mut value: Value = serde_json::from_str(FIXTURE).expect("known context fixture");
    value["records"][0]["last_reviewed"] = json!(workbench_core::result::timestamp());
    value
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "workbench-context-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn source(&self, value: &Value) -> PathBuf {
        let path = self.0.join("source.json");
        fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn invoke(source: &Path, arguments: &[&str], code: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_save"))
        .args(["--json", "--context-source"])
        .arg(source)
        .args(["--context-max-age-days", "30"])
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["record_mode"], "never");
    assert_eq!(result["assessment"], "not_assessed");
    assert!(result["resolved_target"].as_object().unwrap().is_empty());
    result
}
fn resolve(source: &Path, code: i32) -> Value {
    invoke(
        source,
        &[
            "context",
            "resolve",
            "--service",
            "checkout",
            "--env",
            "dev",
        ],
        code,
    )
}
fn error(result: &Value, code: &str) {
    assert_eq!(result["errors"][0]["code"], code);
    assert_eq!(result["effect_outcome"], "not_attempted");
}

fn clock_normalized(mut data: Value) -> Value {
    let age = data["resolution"]["age_seconds"]
        .as_u64()
        .expect("nonnegative age");
    let days = data["resolution"]["max_age_days"].as_u64().unwrap();
    assert!(age <= days * 86400);
    let observed = data["source"]["observed_at"]
        .as_str()
        .expect("UTC observation time");
    assert!((20..=64).contains(&observed.len()) && observed.ends_with('Z'));
    for (position, expected) in [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')] {
        assert_eq!(observed.as_bytes()[position], expected);
    }
    assert!(observed.as_bytes()[..19].iter().enumerate().all(|(position,byte)| [4,7,10,13,16].contains(&position) || byte.is_ascii_digit()));
    if observed.len() > 20 {
        assert_eq!(observed.as_bytes()[19], b'.');
        assert!(
            observed.as_bytes()[20..observed.len() - 1]
                .iter()
                .all(u8::is_ascii_digit)
        );
    }
    data["source"]
        .as_object_mut()
        .unwrap()
        .remove("observed_at");
    data["resolution"]
        .as_object_mut()
        .unwrap()
        .remove("age_seconds");
    data
}

#[test]
fn review_r1_positional_source_objects_are_refused() {
    let scratch = Scratch::new();
    for position in [
        "source",
        "root",
        "record",
        "bindings",
        "resource",
        "dependencies",
        "forward",
        "reverse",
    ] {
        let mut fixture: Value = fresh_fixture();
        let record = &mut fixture["records"][0];
        match position {
            "source" => {
                fixture["source"] = json!(["fixture-export", "synthetic-team-export", "fixture-1"])
            }
            "root" => {
                fixture = json!([
                    fixture["schema_version"],
                    fixture["source"],
                    fixture["records"]
                ])
            }
            "record" => {
                let keys = [
                    "service",
                    "team",
                    "environment",
                    "deployment",
                    "state",
                    "last_reviewed",
                    "required_bindings",
                    "bindings",
                    "owners",
                    "resource_aliases",
                    "runbooks",
                    "dependencies",
                ];
                *record = Value::Array(keys.iter().map(|key| record[*key].clone()).collect());
            }
            "bindings" => record["bindings"] = json!(["fixture-checkout", "dev", "synthetic-org"]),
            "resource" => record["resource_aliases"][0] = json!(["grafana.dashboard", "checkout"]),
            "dependencies" => {
                record["dependencies"] = json!([
                    record["dependencies"]["forward"],
                    record["dependencies"]["reverse"]
                ])
            }
            "forward" => {
                record["dependencies"]["forward"][0] =
                    json!(["database", "dev", "data-team", "blue"])
            }
            "reverse" => {
                record["dependencies"]["reverse"] = json!([["frontend", "dev", "web-team", "blue"]])
            }
            _ => unreachable!(),
        }
        error(
            &resolve(&scratch.source(&fixture), 2),
            "invalid_context_source",
        );
    }
}

#[test]
fn review_r1_unsupported_timestamp_lexical_forms_are_refused() {
    let scratch = Scratch::new();
    for timestamp in [
        "2026-10-02X00:00:00Z",
        "2026-10-02T00:00:00z",
        "2026-10-02T00:00:00-00:00",
    ] {
        let mut fixture: Value = fresh_fixture();
        fixture["records"][0]["last_reviewed"] = json!(timestamp);
        error(
            &resolve(&scratch.source(&fixture), 2),
            "invalid_context_review_time",
        );
    }
    let mut fixture = fresh_fixture();
    fixture["records"][0]["last_reviewed"] =
        json!(format!("2026-10-02T00:00:00.{}Z", "0".repeat(50)));
    error(
        &resolve(&scratch.source(&fixture), 2),
        "invalid_context_review_time",
    );
}

#[test]
fn review_r1_valid_utc_lexical_forms_preserve_original_review_text() {
    let scratch = Scratch::new();
    for separator in ['T', 't'] {
        for fraction in ["", ".0000000000000"] {
            for suffix in ["Z", "+00:00"] {
                let mut fixture = fresh_fixture();
                let date = fixture["records"][0]["last_reviewed"].as_str().unwrap()[..19]
                    .replace('T', &separator.to_string());
                let timestamp = format!("{date}{fraction}{suffix}");
                fixture["records"][0]["last_reviewed"] = json!(timestamp);
                let result = resolve(&scratch.source(&fixture), 0);
                assert_eq!(result["data"]["record"]["last_reviewed"], timestamp);
            }
        }
    }
}

#[test]
fn unique_cli_and_call_preserve_source_and_dependency_directions() {
    let scratch = Scratch::new();
    let source = scratch.source(&fresh_fixture());
    let before = fs::read(&source).unwrap();
    let cli = resolve(&source, 0);
    let request = scratch.0.join("request.json");
    fs::write(&request,serde_json::to_vec(&json!({"spec_version":"0.1","request_id":"context-fixture","operation":"context.resolve","operation_version":1,"target":{"kind":"local","id":"workstation"},"inputs":{"service":"checkout","environment":"dev"},"limits":{"timeout_ms":1000,"max_output_bytes":1048576},"record":"never"})).unwrap()).unwrap();
    let call = invoke(
        &source,
        &["call", "--request", request.to_str().unwrap()],
        0,
    );
    assert_eq!(
        clock_normalized(cli["data"].clone()),
        clock_normalized(call["data"].clone())
    );
    assert_eq!(cli["data"]["record"], call["data"]["record"]);
    for key in [
        "state",
        "usable",
        "max_age_days",
        "missing_required",
        "missing_optional",
    ] {
        assert_eq!(
            cli["data"]["resolution"][key],
            call["data"]["resolution"][key]
        );
    }
    assert_eq!(cli["data"]["resolution"]["usable"], true);
    assert_eq!(cli["coverage"]["state"], "complete");
    assert_eq!(cli["effect_outcome"], "not_applicable");
    assert_eq!(cli["sources"][0]["evidence_state"], "sourced");
    let expected = Sha256::digest(&before)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        cli["data"]["source"]["sha256"],
        format!("sha256:{expected}")
    );
    assert_eq!(
        cli["data"]["source"]["sha256"],
        call["data"]["source"]["sha256"]
    );
    assert_eq!(cli["data"]["source"]["kind"], "fixture-export");
    assert_eq!(
        cli["data"]["record"]["last_reviewed"],
        serde_json::from_slice::<Value>(&before).unwrap()["records"][0]["last_reviewed"]
    );
    assert_eq!(
        cli["data"]["record"]["dependencies"]["forward"][0]["service"],
        "database"
    );
    assert_eq!(cli["data"]["record"]["dependencies"]["reverse"], json!([]));
    assert_eq!(fs::read(source).unwrap(), before);
}

#[test]
fn selectors_refuse_ambiguity_and_missing_without_first_match() {
    let scratch = Scratch::new();
    let mut fixture: Value = fresh_fixture();
    let mut other = fixture["records"][0].clone();
    other["team"] = json!("another-team");
    fixture["records"].as_array_mut().unwrap().push(other);
    let source = scratch.source(&fixture);
    let ambiguous = resolve(&source, 2);
    error(&ambiguous, "context_ambiguous");
    assert_eq!(ambiguous["data"], json!({}));
    let selected = invoke(
        &source,
        &[
            "context",
            "resolve",
            "--service",
            "checkout",
            "--env",
            "dev",
            "--team",
            "platform",
            "--deployment",
            "blue",
        ],
        0,
    );
    assert_eq!(selected["data"]["record"]["team"], "platform");
    let missing = invoke(
        &source,
        &[
            "context",
            "resolve",
            "--service",
            "checkout",
            "--env",
            "prod",
        ],
        2,
    );
    error(&missing, "context_not_found");
}

#[test]
fn stale_retired_future_and_required_omissions_never_become_usable() {
    let scratch = Scratch::new();
    for (field, value, code) in [
        ("state", json!("retired"), "context_retired"),
        (
            "last_reviewed",
            json!("2000-01-01T00:00:00Z"),
            "context_stale",
        ),
        (
            "last_reviewed",
            json!("2100-01-01T00:00:00Z"),
            "future_context_review_time",
        ),
    ] {
        let mut fixture: Value = fresh_fixture();
        fixture["records"][0][field] = value;
        let result = resolve(&scratch.source(&fixture), 2);
        error(&result, code);
        assert_ne!(result["data"]["resolution"]["usable"], true);
    }
    let mut fixture: Value = fresh_fixture();
    fixture["records"][0]["bindings"]
        .as_object_mut()
        .unwrap()
        .remove("target");
    let result = resolve(&scratch.source(&fixture), 2);
    error(&result, "context_incomplete_bindings");
    assert_eq!(
        result["data"]["resolution"]["missing_required"],
        json!(["target"])
    );
}

#[test]
fn absent_optional_fields_differ_from_declared_empty_and_text_is_only_data() {
    let scratch = Scratch::new();
    let mut fixture: Value = fresh_fixture();
    fixture["records"][0]
        .as_object_mut()
        .unwrap()
        .remove("owners");
    fixture["records"][0]["dependencies"]
        .as_object_mut()
        .unwrap()
        .remove("forward");
    fixture["records"][0]["runbooks"] = json!([format!(
        "$(touch {})",
        scratch.0.join("never-created").display()
    )]);
    let result = resolve(&scratch.source(&fixture), 0);
    assert_eq!(result["coverage"]["state"], "partial");
    assert_eq!(result["data"]["resolution"]["usable"], true);
    assert!(result["data"]["record"].get("owners").is_none());
    assert!(
        result["data"]["record"]["dependencies"]
            .get("forward")
            .is_none()
    );
    assert_eq!(
        result["data"]["record"]["dependencies"]["reverse"],
        json!([])
    );
    assert_eq!(
        result["data"]["resolution"]["missing_optional"],
        json!(["owners", "dependencies.forward"])
    );
    assert!(!scratch.0.join("never-created").exists());
}

#[test]
fn malformed_unknown_null_and_environment_mismatch_are_refused() {
    let scratch = Scratch::new();
    for (field, value, code) in [
        (
            "schema_version",
            json!(2),
            "unsupported_context_source_version",
        ),
        ("unknown", json!(true), "invalid_context_source"),
    ] {
        let mut fixture: Value = fresh_fixture();
        fixture[field] = value;
        error(&resolve(&scratch.source(&fixture), 2), code);
    }
    for (field, value, code) in [
        ("owners", Value::Null, "invalid_context_source"),
        (
            "last_reviewed",
            json!("invalid"),
            "invalid_context_review_time",
        ),
    ] {
        let mut fixture: Value = fresh_fixture();
        fixture["records"][0][field] = value;
        error(&resolve(&scratch.source(&fixture), 2), code);
    }
    let mut fixture: Value = fresh_fixture();
    fixture["records"][0]["bindings"]["environment"] = json!("prod");
    error(
        &resolve(&scratch.source(&fixture), 2),
        "invalid_context_source",
    );
    let source = scratch.0.join("source.json");
    fs::write(
        &source,
        FIXTURE.replace(
            "\"schema_version\": 1",
            "\"schema_version\": 1, \"schema_version\": 1",
        ),
    )
    .unwrap();
    error(&resolve(&source, 2), "invalid_json");
    fs::write(&source, vec![b' '; 256 * 1024 + 1]).unwrap();
    error(&resolve(&source, 2), "context_source_too_large");
}

#[test]
fn source_descriptor_refuses_fifo_and_every_symlink_component() {
    use std::{ffi::CString, os::unix::fs::symlink};
    let scratch = Scratch::new();
    let source = scratch.source(&fresh_fixture());
    error(
        &resolve(Path::new("/dev/null"), 2),
        "invalid_context_source_file",
    );
    let link = scratch.0.join("link.json");
    symlink(&source, &link).unwrap();
    error(&resolve(&link, 2), "context_source_read_failed");
    let directory_link = scratch.0.join("linked-directory");
    symlink(&scratch.0, &directory_link).unwrap();
    error(
        &resolve(&directory_link.join("source.json"), 2),
        "context_source_read_failed",
    );
    let fifo = scratch.0.join("fifo");
    let name = CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: name is a valid terminated path and mkfifo retains no pointers.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    error(&resolve(&fifo, 2), "invalid_context_source_file");
    let discovery = invoke(&fifo, &["capabilities", "describe", "context.resolve"], 0);
    assert_eq!(
        discovery["data"]["availability"]["status"],
        "requires_configuration"
    );
}

#[test]
fn startup_pairing_and_adapter_refusal_precede_any_listener_or_server() {
    let scratch = Scratch::new();
    let source = scratch.source(&fresh_fixture());
    for args in [
        vec![
            "--context-source",
            source.to_str().unwrap(),
            "context",
            "resolve",
            "--service",
            "checkout",
            "--env",
            "dev",
        ],
        vec![
            "--context-max-age-days",
            "30",
            "context",
            "resolve",
            "--service",
            "checkout",
            "--env",
            "dev",
        ],
        vec![
            "--context-source",
            source.to_str().unwrap(),
            "--context-max-age-days",
            "0",
            "context",
            "resolve",
            "--service",
            "checkout",
            "--env",
            "dev",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_save"))
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
    for adapter in ["ui", "mcp"] {
        let output = Command::new(env!("CARGO_BIN_EXE_save"))
            .args([
                "--context-source",
                source.to_str().unwrap(),
                "--context-max-age-days",
                "30",
                adapter,
                "serve",
            ])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}
