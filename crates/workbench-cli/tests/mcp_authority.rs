#![cfg(all(feature = "test-fixtures", target_os = "linux"))]
#[allow(dead_code)]
#[path = "support/grafana_https.rs"]
mod https;

use https::{Fixture, Reply, TOKEN, has_error};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::Command};
use workbench_core::{
    OperatorContext, RunControl, execute, execute_with_authority_identities, execute_with_context,
    execute_with_policy_identity, grafana_config_description, request::Request, result::Status,
};

// The separate test process supplies synthetic provider variables without unsafely mutating the
// multi-threaded test runner's environment. Its fixture listener remains in the private sandbox.
fn child(fixture: &Fixture, mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .env("AUTHORITY_TEST_CONFIG", &fixture.config)
        .env("AUTHORITY_TEST_MODE", mode)
        .args([
            "--exact",
            "authority_child",
            "--nocapture",
            "--test-threads=1",
        ]);
    command
}

fn run(command: &mut Command) -> Value {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "child failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.split_once("AUTHORITY_RESULT "))
        .map(|(_, value)| serde_json::from_str(value).unwrap())
        .expect("child result")
}

fn request() -> Request {
    let mut request = Request::new("grafana.dashboard.get", json!({"uid":"board-1"}));
    request.target.kind = "connection".into();
    request.target.id = "fixture".into();
    request
}

#[test]
fn authority_child() {
    let Ok(path) = std::env::var("AUTHORITY_TEST_CONFIG") else {
        return;
    };
    let path = PathBuf::from(path);
    let (description, identity) = grafana_config_description(&path).unwrap();
    let mode = std::env::var("AUTHORITY_TEST_MODE").unwrap();
    if mode == "describe" {
        println!(
            "AUTHORITY_RESULT {}",
            json!({"description":description,"description_debug":format!("{description:?}"),"identity_debug":format!("{identity:?}")})
        );
        return;
    }
    if let Ok(replacement) = std::env::var("AUTHORITY_TEST_REPLACE_CONFIG") {
        fs::copy(replacement, &path).unwrap();
    }
    if let Ok(replacement) = std::env::var("AUTHORITY_TEST_REPLACE_CA") {
        let configuration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        fs::copy(
            replacement,
            configuration["connections"]["fixture"]["tls_ca_file"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    }
    if mode == "deleted-config" {
        fs::remove_file(&path).unwrap();
    }
    if mode == "deleted-ca" {
        let configuration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        fs::remove_file(
            configuration["connections"]["fixture"]["tls_ca_file"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    }
    let context = OperatorContext {
        config_path: (mode != "missing-config").then_some(path),
        ..Default::default()
    };
    let mut request = request();
    if let Ok(alias) = std::env::var("AUTHORITY_TEST_TARGET") {
        request.target.id = alias;
    }
    if mode == "query" {
        request.operation = "grafana.query".into();
        request.inputs = json!({"datasource":"metrics-1","kind":"prometheus","from":"2026-10-02T00:00:00Z","to":"2026-10-02T00:01:00Z","expr":"up"});
    }
    let control = RunControl::default();
    let result = execute_with_authority_identities(
        request.clone(),
        &control,
        &context,
        None,
        (mode != "missing-identity").then_some(&identity.clone()),
    );
    let mut output = json!({"pinned":result});
    if mode == "compare" || mode == "query" {
        output["operator"] =
            serde_json::to_value(execute_with_context(request.clone(), &control, &context))
                .unwrap();
        output["policy_wrapper"] = serde_json::to_value(execute_with_policy_identity(
            request, &control, &context, None,
        ))
        .unwrap();
    }
    println!("AUTHORITY_RESULT {output}");
}

fn denied(receipt: &Value, code: &str) {
    assert_eq!(receipt["execution"]["status"], "denied", "{receipt}");
    assert!(has_error(receipt, code), "{receipt}");
    assert_eq!(receipt["effect_outcome"], "not_attempted");
    assert_eq!(receipt["execution"]["child_exit_code"], Value::Null);
    if let Some(transport) = receipt["data"].get("transport") {
        assert_eq!(transport["requests_started"], 0);
        assert_eq!(transport["responses_received"], 0);
        assert_eq!(transport["stage"], "admission");
    }
    assert!(receipt["sources"].as_array().unwrap().is_empty());
}

#[test]
fn description_is_credential_free_and_exposes_only_ids_profile_support_and_raw_digest() {
    let fixture = Fixture::new(vec![]);
    fixture.edit_config(|config| {
        let mut unsupported = config["connections"]["fixture"].clone();
        unsupported["api_profile"] = json!("private-unsupported-profile");
        unsupported["credential_ref"] = json!("env:SAVE_GRAFANA_NEVER_LOOK_UP");
        config["connections"]["unavailable"] = unsupported;
    });
    for credential in [None, Some("this provider is deliberately invalid")] {
        let mut command = child(&fixture, "describe");
        if let Some(credential) = credential {
            command.env("SAVE_GRAFANA_FIXTURE", credential);
        }
        let output = run(&mut command);
        assert_eq!(
            output["description"],
            json!({
            "digest":format!("sha256:{}", Sha256::digest(fs::read(&fixture.config).unwrap()).iter().map(|byte| format!("{byte:02x}")).collect::<String>()),
                "targets":[{"id":"fixture","api_profile":"grafana-legacy-v1"},{"id":"unavailable","api_profile":null}]
            })
        );
        assert_eq!(output["identity_debug"], "GrafanaConfigIdentity { .. }");
        let encoded = output.to_string();
        for private in [
            "SAVE_GRAFANA_",
            "private-unsupported-profile",
            "https://",
            "ca.pem",
            "deliberately invalid",
            TOKEN,
        ] {
            assert!(!encoded.contains(private));
        }
    }
    assert!(fixture.requests().is_empty());
}

#[test]
fn description_reuses_bounded_strict_config_and_ca_admission() {
    let fixture = Fixture::new(vec![]);
    let original = fs::read(&fixture.config).unwrap();
    for (bytes, code) in [
        (b"[]".to_vec(), "invalid_configuration"),
        (
            br#"{"spec_version":"0.1","record":"never","connections":{},"connections":{}}"#
                .to_vec(),
            "invalid_configuration",
        ),
        (
            format!("{}0{}", "[".repeat(17), "]".repeat(17)).into_bytes(),
            "invalid_configuration",
        ),
        (vec![b' '; 64 * 1024 + 1], "configuration_too_large"),
    ] {
        fs::write(&fixture.config, bytes).unwrap();
        assert_eq!(
            grafana_config_description(&fixture.config)
                .unwrap_err()
                .code,
            code
        );
    }
    fs::write(&fixture.config, &original).unwrap();
    fixture.edit_config(|config| {
        let connection = config["connections"]["fixture"].clone();
        config["connections"] = json!({});
        for i in 0..16 {
            config["connections"][format!("target-{i}")] = connection.clone();
        }
    });
    assert_eq!(
        grafana_config_description(&fixture.config)
            .unwrap()
            .0
            .targets
            .len(),
        16
    );
    fixture.edit_config(|config| {
        config["connections"]["too-many"] = config["connections"]["target-0"].clone()
    });
    assert_eq!(
        grafana_config_description(&fixture.config)
            .unwrap_err()
            .code,
        "invalid_configuration"
    );
    fs::write(&fixture.config, original).unwrap();
    for (bytes, code) in [
        (b"not a certificate".to_vec(), "invalid_ca_bundle"),
        (vec![b' '; 64 * 1024 + 1], "configuration_too_large"),
    ] {
        fs::write(&fixture.ca, bytes).unwrap();
        assert_eq!(
            grafana_config_description(&fixture.config)
                .unwrap_err()
                .code,
            code
        );
    }
    fs::remove_file(&fixture.ca).unwrap();
    fs::create_dir(&fixture.ca).unwrap();
    assert_eq!(
        grafana_config_description(&fixture.config)
            .unwrap_err()
            .code,
        "invalid_configuration"
    );
    assert!(fixture.requests().is_empty());
}

#[test]
fn pinned_observations_match_unchanged_operator_and_policy_wrappers() {
    for mode in ["compare", "query"] {
        let replies = (0..3)
            .flat_map(|_| {
                if mode == "query" {
                    vec![
                        Reply::org(),
                        Reply::datasource("prometheus"),
                        Reply::query(),
                    ]
                } else {
                    vec![Reply::org(), Reply::dashboard()]
                }
            })
            .collect();
        let fixture = Fixture::new(replies);
        let output =
            run(child(&fixture, mode).env("SAVE_GRAFANA_FIXTURE", fixture.provider.to_string()));
        for key in ["pinned", "operator", "policy_wrapper"] {
            assert_eq!(output[key]["execution"]["status"], "succeeded", "{output}");
            for field in [
                "operation",
                "target",
                "resolved_target",
                "execution",
                "effect_outcome",
                "assessment",
                "data",
                "errors",
            ] {
                assert_eq!(output[key][field], output["pinned"][field], "{key}.{field}");
            }
        }
        let requests = fixture.requests();
        assert_eq!(requests.len(), if mode == "query" { 9 } else { 6 });
        for request in requests {
            assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
        }
    }
}

#[test]
fn replaced_config_is_denied_even_when_an_independent_provider_would_allow_new_target() {
    let fixture = Fixture::new(vec![]);
    let replacement = Fixture::new(vec![
        Reply::org(),
        Reply::dashboard(),
        Reply::org(),
        Reply::dashboard(),
    ]);
    replacement.edit_config(|config| {
        config["connections"]["fixture"]["credential_ref"] = json!("env:SAVE_GRAFANA_REPLACEMENT")
    });
    let output = run(child(&fixture, "compare")
        .env("AUTHORITY_TEST_REPLACE_CONFIG", &replacement.config)
        .env("SAVE_GRAFANA_REPLACEMENT", replacement.provider.to_string()));
    denied(&output["pinned"], "grafana_configuration_changed");
    assert!(fixture.requests().is_empty());
    for key in ["operator", "policy_wrapper"] {
        assert_eq!(output[key]["execution"]["status"], "succeeded", "{output}");
    }
    assert_eq!(
        replacement.requests().len(),
        4,
        "only the two explicit unpinned controls may send HTTP"
    );
}

#[test]
fn raw_config_replacement_precedes_json_parsing_and_credential_lookup() {
    for bytes in [None, Some(b"[]".to_vec())] {
        let fixture = Fixture::new(vec![]);
        let replacement = fixture.directory.join("replacement.json");
        let bytes = bytes.unwrap_or_else(|| {
            let mut bytes = fs::read(&fixture.config).unwrap();
            bytes.push(b'\n');
            bytes
        });
        fs::write(&replacement, bytes).unwrap();
        let output =
            run(child(&fixture, "pinned").env("AUTHORITY_TEST_REPLACE_CONFIG", replacement));
        denied(&output["pinned"], "grafana_configuration_changed");
        assert!(fixture.requests().is_empty());
    }
}

#[test]
fn selected_ca_bytes_are_checked_before_credentials_and_http() {
    for provide_credential in [false, true] {
        let fixture = Fixture::new(vec![
            Reply::org(),
            Reply::dashboard(),
            Reply::org(),
            Reply::dashboard(),
        ]);
        let replacement = fixture.directory.join("same-cert-new-bytes.pem");
        let mut bytes = fs::read(&fixture.ca).unwrap();
        bytes.push(b'\n');
        fs::write(&replacement, bytes).unwrap();
        let mut command = child(
            &fixture,
            if provide_credential {
                "compare"
            } else {
                "pinned"
            },
        );
        command.env("AUTHORITY_TEST_REPLACE_CA", replacement);
        if provide_credential {
            command.env("SAVE_GRAFANA_FIXTURE", fixture.provider.to_string());
        }
        let output = run(&mut command);
        denied(&output["pinned"], "grafana_ca_changed");
        if provide_credential {
            for key in ["operator", "policy_wrapper"] {
                assert_eq!(output[key]["execution"]["status"], "succeeded", "{output}");
            }
        }
        assert_eq!(
            fixture.requests().len(),
            if provide_credential { 4 } else { 0 }
        );
    }
}

#[test]
fn missing_context_and_removed_authority_files_refuse_without_effects() {
    for (mode, code) in [
        ("missing-identity", "grafana_identity_required"),
        ("missing-config", "grafana_configuration_required"),
        ("deleted-config", "grafana_configuration_changed"),
        ("deleted-ca", "grafana_ca_changed"),
    ] {
        let fixture = Fixture::new(vec![]);
        let output =
            run(child(&fixture, mode).env("SAVE_GRAFANA_FIXTURE", fixture.provider.to_string()));
        denied(&output["pinned"], code);
        assert!(fixture.requests().is_empty());
    }
}

#[test]
fn pinned_authority_preserves_provider_scope_and_target_checks() {
    for (target, changed_scope, code) in [
        ("other", false, "unknown_connection"),
        ("fixture", true, "credential_scope_mismatch"),
    ] {
        let mut fixture = Fixture::new(vec![]);
        if changed_scope {
            fixture.provider["operations"] = json!(["grafana.query"]);
        }
        let output = run(child(&fixture, "pinned")
            .env("AUTHORITY_TEST_TARGET", target)
            .env("SAVE_GRAFANA_FIXTURE", fixture.provider.to_string()));
        denied(&output["pinned"], code);
        assert!(fixture.requests().is_empty());
    }
}

#[test]
fn commands_require_identity_but_numerical_tasks_and_legacy_wrappers_do_not() {
    let directory = std::env::temp_dir().join(workbench_core::result::new_id("authority-marker"));
    fs::create_dir(&directory).unwrap();
    let marker = directory.join("must-not-exist");
    for operation in ["process.exec", "command.inspect"] {
        let request = Request::new(
            operation,
            json!({"program":"/usr/bin/touch","args":[marker],"cwd":"/tmp"}),
        );
        for path in [None, Some(directory.join("must-not-read.json"))] {
            let result = execute_with_authority_identities(
                request.clone(),
                &RunControl::default(),
                &OperatorContext {
                    read_policy_path: path,
                    ..Default::default()
                },
                None,
                None,
            );
            denied(
                &serde_json::to_value(result).unwrap(),
                "read_policy_identity_required",
            );
        }
        let result = execute_with_authority_identities(
            request,
            &RunControl::default(),
            &OperatorContext::default(),
            Some("sha256:pinned"),
            None,
        );
        denied(
            &serde_json::to_value(result).unwrap(),
            "read_policy_required",
        );
    }
    assert!(!marker.exists());
    let request = Request::new(
        "command.inspect",
        json!({"program":"/usr/bin/printf","args":["unchanged"],"cwd":"/tmp"}),
    );
    assert_eq!(
        execute(request.clone(), &RunControl::default())
            .execution
            .status,
        Status::Succeeded
    );
    assert_eq!(
        execute_with_context(
            request.clone(),
            &RunControl::default(),
            &OperatorContext::default()
        )
        .execution
        .status,
        Status::Succeeded
    );
    assert_eq!(
        execute_with_policy_identity(
            request,
            &RunControl::default(),
            &OperatorContext::default(),
            None
        )
        .execution
        .status,
        Status::Succeeded
    );
    let task = Request::new(
        "task.run",
        json!({"id":"error-budget","version":1,"input":{"slo":99.9}}),
    );
    let result = execute_with_authority_identities(
        task,
        &RunControl::default(),
        &OperatorContext::default(),
        None,
        None,
    );
    assert_eq!(result.execution.status, Status::Succeeded, "{result:?}");
    fs::remove_dir_all(directory).unwrap();
}
