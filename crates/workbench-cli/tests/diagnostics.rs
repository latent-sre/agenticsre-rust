#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

struct Files {
    root: PathBuf,
    config: PathBuf,
    input: PathBuf,
}
impl Files {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "workbench-diagnostics-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let config = root.join("config.json");
        let input = root.join("input.json");
        fs::write(&input, b"{}").unwrap();
        Self {
            root,
            config,
            input,
        }
    }
    fn cli(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_save"));
        command
            .args(["--json", "--diagnostics-config"])
            .arg(&self.config)
            .args([
                "diagnose",
                "--target",
                "fixture-web",
                "--pack",
                "connectivity",
                "--input",
            ])
            .arg(&self.input);
        command
    }
    fn request(&self, inputs: Value) -> Command {
        let path = self.root.join("request.json");
        fs::write(&path, serde_json::to_vec(&json!({"spec_version":"0.1","request_id":"diagnostics-test","operation":"diagnostics.run","operation_version":1,
            "target":{"kind":"connection","id":"fixture-web"},"inputs":inputs,
            "limits":{"timeout_ms":500,"max_output_bytes":16384,"concurrency":2},"record":"never"})).unwrap()).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_save"));
        command
            .args(["--json", "--diagnostics-config"])
            .arg(&self.config)
            .args(["call", "--request"])
            .arg(path);
        command
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn receipt(output: &Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["assessment"], "not_assessed");
    assert_eq!(value["record_mode"], "never");
    assert_eq!(value["execution"]["child_exit_code"], Value::Null);
    value
}
fn configuration() -> Value {
    json!({"schema_version":1,"targets":[{"id":"fixture-web","host":"diagnostics.workbench.test","port":8443,"path":"/probe",
        "allowed_ips":["127.0.0.1"],"resolver":{"kind":"explicit","address":"127.0.0.1","port":5353,"transport":"udp"}}]})
}

#[test]
fn discovery_does_not_open_the_configured_fifo_or_probe_a_process() {
    let files = Files::new();
    let path = std::ffi::CString::new(files.config.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: path is a valid NUL-terminated filename in this test's owned directory.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    for arguments in [
        vec!["diagnostics", "list"],
        vec!["diagnostics", "describe", "connectivity"],
        vec!["capabilities", "describe", "diagnostics.run"],
        vec!["doctor"],
    ] {
        let result = receipt(
            &Command::new(env!("CARGO_BIN_EXE_save"))
                .arg("--json")
                .arg("--diagnostics-config")
                .arg(&files.config)
                .args(arguments)
                .output()
                .unwrap(),
            0,
        );
        assert_eq!(result["effect_outcome"], "not_applicable");
        assert!(!result["data"].to_string().contains("configuration_sha256"));
    }
    let result = receipt(&files.cli().output().unwrap(), 2);
    assert_eq!(result["errors"][0]["code"], "invalid_diagnostics_file");
    assert_eq!(result["effect_outcome"], "not_attempted");
}

#[test]
fn wire_bindings_and_positional_pack_objects_are_refused_before_configuration_io() {
    let files = Files::new(); // Config intentionally does not exist.
    for inputs in [
        json!({"id":"connectivity","version":1,"input":[false,"continue"]}),
        json!({"id":"connectivity","version":1,"input":{"host":"private.invalid"}}),
        json!({"id":"connectivity","version":1,"input":{"capture_body":null}}),
        json!({"id":"connectivity","version":1,"input":{"on_failure":"fallback"}}),
        json!({"id":"connectivity","version":1,"input":{"resolver":"127.0.0.1:1"}}),
        json!({"id":"connectivity","version":1,"input":{"pid":1}}),
    ] {
        let result = receipt(&files.request(inputs).output().unwrap(), 2);
        assert_eq!(result["errors"][0]["code"], "invalid_diagnostics_input");
        assert_eq!(result["resolved_target"], json!({}));
    }
    let result = receipt(
        &files
            .request(json!({"id":"connectivity","version":2,"input":{}}))
            .output()
            .unwrap(),
        2,
    );
    assert_eq!(result["errors"][0]["code"], "unknown_diagnostics_pack");
}

#[test]
fn configuration_objects_nulls_duplicates_and_hostile_destinations_cannot_dispatch() {
    let files = Files::new();
    let base = configuration();
    let mut bad = Vec::new();
    for (key, value) in [
        ("host", json!("127.0.0.01")),
        ("host", json!("localhost")),
        ("host", json!("::ffff:127.0.0.1")),
        ("path", json!("/probe?secret=value")),
        ("path", json!("/../probe")),
        ("path", json!("/a%2fb")),
        ("port", json!(0)),
        ("resolver", json!(["explicit", "127.0.0.1", 5353, "udp"])),
        ("process", json!(["linux-process-v1", 1, "0", 1000])),
        ("process", Value::Null),
        ("tls_ca_file", Value::Null),
        ("allowed_ips", json!(["127.0.0.1", "127.0.0.1"])),
    ] {
        let mut config = base.clone();
        config["targets"][0][key] = value;
        bad.push(config);
    }
    let mut duplicate = base.clone();
    duplicate["targets"]
        .as_array_mut()
        .unwrap()
        .push(base["targets"][0].clone());
    bad.push(duplicate);
    for config in bad {
        fs::write(&files.config, serde_json::to_vec(&config).unwrap()).unwrap();
        let result = receipt(&files.cli().output().unwrap(), 2);
        assert_eq!(
            result["errors"][0]["code"], "invalid_diagnostics_configuration",
            "{config}"
        );
        assert_eq!(result["effect_outcome"], "not_attempted");
        assert!(result["sources"].as_array().unwrap().is_empty());
    }
    fs::write(
        &files.config,
        b"{\"schema_version\":1,\"schema_version\":1,\"targets\":[]}",
    )
    .unwrap();
    let result = receipt(&files.cli().output().unwrap(), 2);
    assert_eq!(
        result["errors"][0]["code"],
        "invalid_diagnostics_configuration"
    );
}

#[test]
fn configuration_and_ca_symlinks_are_refused_at_every_component() {
    use std::os::unix::fs::symlink;
    let files = Files::new();
    let real = files.root.join("regular.json");
    fs::write(&real, serde_json::to_vec(&configuration()).unwrap()).unwrap();
    symlink(&real, &files.config).unwrap();
    assert_eq!(
        receipt(&files.cli().output().unwrap(), 2)["errors"][0]["code"],
        "diagnostics_file_read_failed"
    );
    fs::remove_file(&files.config).unwrap();
    let nested = files.root.join("alias");
    symlink(&files.root, &nested).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_save"));
    command
        .args(["--json", "--diagnostics-config"])
        .arg(nested.join("regular.json"))
        .args(["diagnose", "--target", "fixture-web", "--input"])
        .arg(&files.input);
    assert_eq!(
        receipt(&command.output().unwrap(), 2)["errors"][0]["code"],
        "diagnostics_file_read_failed"
    );
    let mut config = configuration();
    config["targets"][0]["tls_ca_file"] = json!(nested.join("regular.json").to_str().unwrap());
    fs::write(&files.config, serde_json::to_vec(&config).unwrap()).unwrap();
    assert_eq!(
        receipt(&files.cli().output().unwrap(), 2)["errors"][0]["code"],
        "diagnostics_file_read_failed"
    );
}

#[test]
fn missing_alias_and_hosted_startup_grants_do_not_fall_back() {
    let files = Files::new();
    let mut config = configuration();
    config["targets"][0]["id"] = json!("other");
    fs::write(&files.config, serde_json::to_vec(&config).unwrap()).unwrap();
    assert_eq!(
        receipt(&files.cli().output().unwrap(), 2)["errors"][0]["code"],
        "diagnostics_target_not_found"
    );
    for adapter in ["ui", "mcp"] {
        let output = Command::new(env!("CARGO_BIN_EXE_save"))
            .arg("--diagnostics-config")
            .arg(&files.config)
            .args([adapter, "serve"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn configuration_and_ca_bounds_refuse_nonregular_and_oversized_sources() {
    let files = Files::new();
    fs::write(&files.config, vec![b' '; 65537]).unwrap();
    assert_eq!(
        receipt(&files.cli().output().unwrap(), 2)["errors"][0]["code"],
        "diagnostics_file_too_large"
    );
    let ca = files.root.join("ca.pem");
    let mut config = configuration();
    config["targets"][0]["tls_ca_file"] = json!(ca.to_str().unwrap());
    fs::write(&files.config, serde_json::to_vec(&config).unwrap()).unwrap();
    fs::write(&ca, vec![b' '; 65537]).unwrap();
    assert_eq!(
        receipt(&files.cli().output().unwrap(), 2)["errors"][0]["code"],
        "diagnostics_file_too_large"
    );
    fs::remove_file(&ca).unwrap();
    let name = std::ffi::CString::new(ca.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: name is a valid filename in the owned fixture directory.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert_eq!(
        receipt(&files.cli().output().unwrap(), 2)["errors"][0]["code"],
        "invalid_diagnostics_file"
    );
    config["targets"][0]["tls_ca_file"] = json!("/dev/null");
    fs::write(&files.config, serde_json::to_vec(&config).unwrap()).unwrap();
    assert_eq!(
        receipt(&files.cli().output().unwrap(), 2)["errors"][0]["code"],
        "invalid_diagnostics_file"
    );
    config["targets"][0]["tls_ca_file"] = json!("relative.pem");
    fs::write(&files.config, serde_json::to_vec(&config).unwrap()).unwrap();
    assert_eq!(
        receipt(&files.cli().output().unwrap(), 2)["errors"][0]["code"],
        "invalid_diagnostics_configuration"
    );
    config["targets"][0]["tls_ca_file"] = json!(files.root.join("../ca.pem").to_str().unwrap());
    fs::write(&files.config, serde_json::to_vec(&config).unwrap()).unwrap();
    assert_eq!(
        receipt(&files.cli().output().unwrap(), 2)["errors"][0]["code"],
        "invalid_diagnostics_file_path"
    );
}

#[test]
fn declaration_cardinality_and_ca_syntax_are_admitted_before_network_work() {
    let files = Files::new();
    let mut config = configuration();
    config["targets"][0]["allowed_ips"] =
        json!((1..=17).map(|n| format!("127.0.0.{n}")).collect::<Vec<_>>());
    fs::write(&files.config, serde_json::to_vec(&config).unwrap()).unwrap();
    assert_eq!(
        receipt(&files.cli().output().unwrap(), 2)["errors"][0]["code"],
        "invalid_diagnostics_configuration"
    );
    config = configuration();
    config["targets"] = json!(
        (0..17)
            .map(|n| {
                let mut target = configuration()["targets"][0].clone();
                target["id"] = json!(format!("target-{n}"));
                target
            })
            .collect::<Vec<_>>()
    );
    fs::write(&files.config, serde_json::to_vec(&config).unwrap()).unwrap();
    assert_eq!(
        receipt(&files.cli().output().unwrap(), 2)["errors"][0]["code"],
        "invalid_diagnostics_configuration"
    );
    config = configuration();
    let ca = files.root.join("ca.pem");
    config["targets"][0]["tls_ca_file"] = json!(ca.to_str().unwrap());
    fs::write(&files.config, serde_json::to_vec(&config).unwrap()).unwrap();
    for bytes in [
        "",
        "private non-PEM text",
        "-----BEGIN CERTIFICATE-----\n!!!\n-----END CERTIFICATE-----",
        "-----BEGIN CERTIFICATE-----\nYQ==\n-----END CERTIFICATE-----",
    ] {
        fs::write(&ca, bytes).unwrap();
        let result = receipt(&files.cli().output().unwrap(), 2);
        assert_eq!(result["errors"][0]["code"], "invalid_diagnostics_ca");
        assert_eq!(result["effect_outcome"], "not_attempted");
    }
}
