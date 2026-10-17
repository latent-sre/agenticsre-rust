#![cfg(target_os = "linux")]
use base64::{Engine, engine::general_purpose::STANDARD};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    net::SocketAddr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread::{self, JoinHandle},
    time::Duration,
};

const SAVE: &str = env!("CARGO_BIN_EXE_save");
const ACTIONS: [&str; 5] = ["plan", "status", "approve", "apply", "reconcile"];
struct Input(PathBuf);
impl Input {
    fn new(value: &Value) -> Self {
        let path = std::env::temp_dir().join(workbench_core::result::new_id("change-input"));
        fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        Self(path)
    }
    fn call(&self, action: &str, record: &str) -> Value {
        self.call_with_args(action, record, &[])
    }
    fn call_with_args(&self, action: &str, record: &str, extra: &[&str]) -> Value {
        let output = Command::new(SAVE)
            .env_clear()
            .args([
                "--json",
                "--record",
                record,
                "change",
                action,
                "--runner",
                "fixture.runner",
                "--input",
            ])
            .arg(&self.0)
            .args(extra)
            .output()
            .unwrap();
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(2), "{value}");
        assert_eq!(value["assessment"], "not_assessed");
        assert_eq!(value["effect_outcome"], "not_attempted");
        value
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn body(action: &str) -> Value {
    let pin = format!("sha256:{}", "0".repeat(64));
    match action {
        "plan" => {
            json!({"request_id":"request.fixture","target":"rules.fixture","proposed_base64":STANDARD.encode(b"groups: []\n"),"expires_in_seconds":300})
        }
        "status" | "reconcile" => {
            json!({"requester":"principal.fixture","request_id":"request.fixture"})
        }
        "approve" => {
            json!({"requester":"principal.fixture","request_id":"request.fixture","plan_sha256":pin})
        }
        "apply" => json!({"request_id":"request.fixture","plan_sha256":pin,"approval_id":pin}),
        _ => unreachable!(),
    }
}
#[test]
fn public_change_commands_require_explicit_runner_authority() {
    for action in ACTIONS {
        let result = Input::new(&body(action)).call(
            action,
            if action == "status" {
                "never"
            } else {
                "always"
            },
        );
        assert_eq!(result["operation"], format!("change.{action}"), "{result}");
        assert_eq!(
            result["target"],
            json!({"kind":"runner","id":"fixture.runner"})
        );
        assert!(
            result["errors"]
                .as_array()
                .unwrap()
                .iter()
                .all(|error| error["code"] != "invalid_usage"
                    && error["code"] != "unsupported_operation"),
            "{result}"
        );
    }
}
#[test]
fn change_commands_preserve_explicit_timeout_and_output_bounds() {
    for action in ACTIONS {
        let result = Input::new(&body(action)).call_with_args(
            action,
            if action == "status" {
                "never"
            } else {
                "always"
            },
            &["--timeout-ms", "1234", "--max-output-bytes", "4096"],
        );
        assert_eq!(result["operation"], format!("change.{action}"), "{result}");
        assert_eq!(result["effective_limits"]["timeout_ms"], 1234);
        assert_eq!(result["effective_limits"]["max_output_bytes"], 4096);
        assert!(
            result["errors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|error| error["code"] == "runner_config_required"),
            "{result}"
        );
    }
}
#[test]
fn change_writes_require_explicit_durable_recording() {
    for action in ["plan", "approve", "apply", "reconcile"] {
        for record in ["never", "auto"] {
            let result = Input::new(&body(action)).call(action, record);
            assert!(
                result["errors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|error| error["code"] == "recording_required"),
                "{result}"
            );
        }
    }
}
#[test]
fn wire_authority_labels_and_noncanonical_proposals_are_refused() {
    for (key, value) in [
        ("approved", json!(true)),
        ("approver", json!("principal.approver")),
        ("path", json!("/tmp/arbitrary.yml")),
        ("proposed_base64", json!("Zh==")),
        ("proposed_base64", json!(STANDARD.encode(vec![b'x'; 32769]))),
        ("expires_in_seconds", json!(901)),
        ("expires_in_seconds", Value::Null),
    ] {
        let mut input = body("plan");
        input[key] = value;
        let result = Input::new(&input).call("plan", "always");
        assert_eq!(result["execution"]["status"], "denied", "{result}");
        assert!(
            result["errors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|error| error["code"] == "invalid_change_input"),
            "invalid inputs must be rejected before profile access: {result}"
        );
    }
}
#[test]
fn every_change_operation_is_discoverable_offline() {
    for action in ACTIONS {
        let operation = format!("change.{action}");
        let output = Command::new(SAVE)
            .env_clear()
            .args(["--json", "capabilities", "describe", &operation])
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(0), "{value}");
        let advertised = value["data"].to_string();
        assert!(
            advertised.contains(&format!("change-{action}-input")),
            "{value}"
        );
        assert_eq!(value["effect_outcome"], "not_applicable");
    }
}

const RUNNER: &str = "fixture.runner";
struct Server {
    child: Child,
    stdout: Option<JoinHandle<Vec<u8>>>,
    stderr: Option<JoinHandle<Vec<u8>>>,
    endpoint: SocketAddr,
    stopped: bool,
}
impl Server {
    fn start(config: &Path) -> Self {
        let mut child = Command::new(SAVE)
            .env_clear()
            .args([
                "--json",
                "--record",
                "always",
                "runner",
                "serve",
                "--server-config",
            ])
            .arg(config)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (sender, ready) = mpsc::channel();
        let stdout = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = Vec::new();
            (&mut reader)
                .take(64 * 1024)
                .read_until(b'\n', &mut line)
                .unwrap();
            let _ = sender.send(line);
            let mut remainder = Vec::new();
            reader
                .take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut remainder)
                .unwrap();
            remainder
        });
        let stderr = thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr
                .take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .unwrap();
            bytes
        });
        let mut server = Self {
            child,
            stdout: Some(stdout),
            stderr: Some(stderr),
            endpoint: "127.0.0.1:0".parse().unwrap(),
            stopped: false,
        };
        let bytes = ready
            .recv_timeout(Duration::from_secs(10))
            .expect("bounded runner readiness");
        let ready: Value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            panic!(
                "invalid readiness {error}: {}",
                String::from_utf8_lossy(&bytes)
            )
        });
        assert_eq!(ready["runner_id"], RUNNER);
        assert_eq!(ready["protocol"], 1);
        server.endpoint = ready["address"].as_str().unwrap().parse().unwrap();
        server
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if !self.stopped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(stdout) = self.stdout.take() {
            let _ = stdout.join();
        }
        if let Some(stderr) = self.stderr.take() {
            let _ = stderr.join();
        }
    }
}

fn private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}
fn receipt(output: Output, expected: i32) -> Value {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(expected), "{value}");
    assert_eq!(value["assessment"], "not_assessed");
    value
}
struct RunnerFixture {
    root: PathBuf,
    server: Server,
    client: Value,
    target: PathBuf,
}
impl RunnerFixture {
    fn restart(&mut self) {
        self.server.child.kill().unwrap();
        self.server.child.wait().unwrap();
        self.server = Server::start(&self.root.join("server.json"));
        self.client["endpoint"] = json!(self.server.endpoint.to_string());
    }
    fn new(promtool: &str) -> Self {
        let root = std::env::temp_dir().join(workbench_core::result::new_id("change-tls"));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let target = root.join("alerts.yml");
        private(
            &target,
            include_bytes!("../../../tests/fixtures/prometheus-rules/clean.yml"),
        );
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca = CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap();
        private(&root.join("ca.der"), ca.der());
        let mut certs = Vec::new();
        for (index, name) in [
            "runner.test",
            "requester.test",
            "approver.test",
            "stranger.test",
        ]
        .into_iter()
        .enumerate()
        {
            let key = KeyPair::generate().unwrap();
            let mut params = CertificateParams::new(vec![name.to_owned()]).unwrap();
            params.extended_key_usages = vec![if index == 0 {
                ExtendedKeyUsagePurpose::ServerAuth
            } else {
                ExtendedKeyUsagePurpose::ClientAuth
            }];
            let cert = params.signed_by(&key, &ca).unwrap();
            private(&root.join(format!("{index}.der")), cert.der());
            private(&root.join(format!("{index}.key")), &key.serialize_der());
            certs.push(digest(cert.der()));
        }
        let check = Input::new(
            &json!({"kind":"prometheus-rules","validator":{"id":"promtool","version":1},"file":target}),
        );
        let checked = receipt(
            Command::new(SAVE)
                .env_clear()
                .args([
                    "--json",
                    "--promtool",
                    promtool,
                    "config",
                    "check",
                    "--input",
                ])
                .arg(&check.0)
                .output()
                .unwrap(),
            0,
        );
        assert_eq!(checked["data"]["validation_outcome"], "valid");
        let config = json!({"config_version":1,"runner_id":RUNNER,"listen":"127.0.0.1:0","state_dir":root.join("jobs"),"ca_certificate":root.join("ca.der"),"certificate":root.join("0.der"),"private_key":root.join("0.key"),"principals":[{"id":"requester.fixture","certificate_sha256":certs[1]},{"id":"approver.fixture","certificate_sha256":certs[2]},{"id":"stranger.fixture","certificate_sha256":certs[3]}],"targets":[{"id":"fixture.target","snapshot":{"name":"Fixture","environment":"isolated-test"}}],"grants":[],"promtool_path":promtool,"changes":{"state_dir":root.join("changes"),"targets":[{"id":"rules.fixture","path":target,"validator_binding_sha256":checked["data"]["validator"]["binding_digest"],"requesters":["requester.fixture"],"approvers":["approver.fixture"]}]}});
        let path = root.join("server.json");
        private(&path, &serde_json::to_vec(&config).unwrap());
        let server = Server::start(&path);
        let client = json!({"config_version":1,"runner_id":RUNNER,"endpoint":server.endpoint.to_string(),"server_name":"runner.test","server_certificate_sha256":certs[0],"ca_certificate":root.join("ca.der"),"certificate":root.join("1.der"),"private_key":root.join("1.key")});
        Self {
            root,
            server,
            client,
            target,
        }
    }
    fn call(&self, principal: usize, action: &str, body: &Value, code: i32) -> Value {
        let mut client = self.client.clone();
        client["certificate"] = json!(self.root.join(format!("{principal}.der")));
        client["private_key"] = json!(self.root.join(format!("{principal}.key")));
        let profile = self.root.join("client.json");
        private(&profile, &serde_json::to_vec(&client).unwrap());
        let input = Input::new(body);
        receipt(
            Command::new(SAVE)
                .env_clear()
                .args([
                    "--json",
                    "--record",
                    if action == "status" {
                        "never"
                    } else {
                        "always"
                    },
                    "--runner-config",
                ])
                .arg(profile)
                .args(["change", action, "--runner", RUNNER, "--input"])
                .arg(input.0.as_path())
                .output()
                .unwrap(),
            code,
        )
    }
}
impl Drop for RunnerFixture {
    fn drop(&mut self) {
        let _ = self.server.child.kill();
        let _ = self.server.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
#[ignore = "Requires the SHA-256-verified official promtool fixture via WORKBENCH_TEST_PROMTOOL"]
fn mtls_exact_approval_replacement_readback_and_replay_preserve_original_attempt() {
    let binary = std::env::var("WORKBENCH_TEST_PROMTOOL").expect("verified promtool fixture");
    let mut f = RunnerFixture::new(&binary);
    let original = fs::read(&f.target).unwrap();
    let proposed = String::from_utf8(original.clone())
        .unwrap()
        .replace("for: 5m", "for: 10m")
        .into_bytes();
    let plan_body = json!({"request_id":"change.fixture","target":"rules.fixture","proposed_base64":STANDARD.encode(&proposed),"expires_in_seconds":300});
    let planned = f.call(1, "plan", &plan_body, 0);
    assert_eq!(planned["data"]["state"], "planned");
    assert_eq!(fs::read(&f.target).unwrap(), original);
    assert_eq!(
        planned["data"]["plan"]["original_base64"],
        STANDARD.encode(&original)
    );
    let lookup = json!({"requester":"requester.fixture","request_id":"change.fixture"});
    let approval = json!({"requester":"requester.fixture","request_id":"change.fixture","plan_sha256":planned["data"]["plan_sha256"]});
    f.call(1, "approve", &approval, 2);
    f.call(3, "status", &lookup, 2);
    let mut wrong = approval.clone();
    wrong["plan_sha256"] = json!(format!("sha256:{}", "0".repeat(64)));
    f.call(2, "approve", &wrong, 2);
    let approved = f.call(2, "approve", &approval, 0);
    assert_eq!(approved["data"]["state"], "approved");
    assert_eq!(fs::read(&f.target).unwrap(), original);
    let apply = json!({"request_id":"change.fixture","plan_sha256":planned["data"]["plan_sha256"],"approval_id":approved["data"]["approval_id"]});
    let applied = f.call(1, "apply", &apply, 0);
    assert_eq!(applied["data"]["state"], "verified");
    assert_eq!(applied["data"]["effect_outcome"], "verified");
    assert_eq!(fs::read(&f.target).unwrap(), proposed);
    let inode = std::os::unix::fs::MetadataExt::ino(&fs::metadata(&f.target).unwrap());
    let replay = f.call(1, "apply", &apply, 0);
    assert_eq!(replay["data"]["state"], "verified");
    let mut wrong_replay = apply.clone();
    wrong_replay["plan_sha256"] = json!(format!("sha256:{}", "0".repeat(64)));
    let refused = f.call(1, "apply", &wrong_replay, 2);
    assert_eq!(refused["effect_outcome"], "not_attempted");
    assert_eq!(refused["data"]["state"], "verified");
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(&fs::metadata(&f.target).unwrap()),
        inode
    );
    let observed = f.call(2, "reconcile", &lookup, 0);
    assert_eq!(observed["data"]["state"], "verified");
    assert_eq!(observed["data"]["plan"], planned["data"]["plan"]);
    assert_eq!(f.call(1, "status", &lookup, 0)["data"]["state"], "verified");
    f.restart();
    assert_eq!(
        f.call(1, "reconcile", &lookup, 0)["data"]["state"],
        "verified"
    );
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(&fs::metadata(&f.target).unwrap()),
        inode
    );
    let recovery = json!({"request_id":"change.recovery","target":"rules.fixture","proposed_base64":planned["data"]["plan"]["original_base64"],"expires_in_seconds":300});
    let recovery_plan = f.call(1, "plan", &recovery, 0);
    assert_eq!(fs::read(&f.target).unwrap(), proposed);
    let recovery_approval=f.call(2,"approve",&json!({"requester":"requester.fixture","request_id":"change.recovery","plan_sha256":recovery_plan["data"]["plan_sha256"]}),0);
    let restored=f.call(1,"apply",&json!({"request_id":"change.recovery","plan_sha256":recovery_plan["data"]["plan_sha256"],"approval_id":recovery_approval["data"]["approval_id"]}),0);
    assert_eq!(restored["data"]["state"], "verified");
    assert_eq!(fs::read(&f.target).unwrap(), original);
    fs::write(&f.target, b"changed after recovery").unwrap();
    let unknown = f.call(
        1,
        "reconcile",
        &json!({"requester":"requester.fixture","request_id":"change.recovery"}),
        1,
    );
    assert_eq!(unknown["effect_outcome"], "unknown");
    assert!(
        !unknown["errors"].as_array().unwrap().is_empty(),
        "unknown receipts must explain the uncertainty"
    );
}

#[test]
#[ignore = "Requires the SHA-256-verified official promtool fixture via WORKBENCH_TEST_PROMTOOL"]
fn mtls_changed_original_refuses_approved_dispatch() {
    let binary = std::env::var("WORKBENCH_TEST_PROMTOOL").expect("verified promtool fixture");
    let f = RunnerFixture::new(&binary);
    let original = fs::read(&f.target).unwrap();
    let proposed = String::from_utf8(original.clone())
        .unwrap()
        .replace("for: 5m", "for: 10m");
    let planned=f.call(1,"plan",&json!({"request_id":"change.altered","target":"rules.fixture","proposed_base64":STANDARD.encode(&proposed),"expires_in_seconds":300}),0);
    let approved=f.call(2,"approve",&json!({"requester":"requester.fixture","request_id":"change.altered","plan_sha256":planned["data"]["plan_sha256"]}),0);
    let altered = [original.as_slice(), b"\n# independently changed\n"].concat();
    fs::write(&f.target, &altered).unwrap();
    let refused=f.call(1,"apply",&json!({"request_id":"change.altered","plan_sha256":planned["data"]["plan_sha256"],"approval_id":approved["data"]["approval_id"]}),2);
    assert_eq!(refused["effect_outcome"], "not_attempted");
    assert_eq!(fs::read(&f.target).unwrap(), altered);
}
