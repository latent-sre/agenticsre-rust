#![cfg(all(feature = "test-fixtures", target_os = "linux"))]
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};
use workbench_core::request::Request;

const SAVE: &str = env!("CARGO_BIN_EXE_save");
const RUNNER: &str = env!("CARGO_BIN_EXE_runner-fixture");
const CAPACITY: &str = "ext.example.capacity.project";

fn digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}
fn receipt(output: Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.len() <= 2 * 1024 * 1024);
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stdout)));
    assert_eq!(value["assessment"], "not_assessed");
    value
}
fn error(value: &Value, code: &str) {
    assert!(
        value["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == code),
        "{value}"
    );
}
fn wait_path(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0) {
        assert!(
            Instant::now() < deadline,
            "missing fixture marker {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}
struct Worker(Option<Child>);
impl Worker {
    fn wait(mut self) -> Output {
        let child = self.0.as_mut().unwrap();
        // Drain both pipes while waiting: a retained noisy receipt can exceed
        // a pipe buffer even though the child operation has already finished.
        let stdout = child.stdout.take().map(|pipe| {
            thread::spawn(move || {
                let mut bytes = Vec::new();
                pipe.take(2 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .unwrap();
                bytes
            })
        });
        let stderr = child.stderr.take().map(|pipe| {
            thread::spawn(move || {
                let mut bytes = Vec::new();
                pipe.take(2 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .unwrap();
                bytes
            })
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        while child.try_wait().unwrap().is_none() {
            assert!(
                Instant::now() < deadline,
                "job worker exceeded bounded fixture lifetime"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let status = self.0.take().unwrap().wait().unwrap();
        Output {
            status,
            stdout: stdout
                .map(|reader| reader.join().unwrap())
                .unwrap_or_default(),
            stderr: stderr
                .map(|reader| reader.join().unwrap())
                .unwrap_or_default(),
        }
    }
    fn disconnect_stdout(&mut self) {
        drop(self.0.as_mut().unwrap().stdout.take());
    }
    fn kill(&mut self) {
        self.0.as_mut().unwrap().kill().unwrap();
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
struct Fixture {
    root: PathBuf,
    store: PathBuf,
    extension: Option<(PathBuf, PathBuf)>,
    ceiling: Option<u64>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(workbench_core::result::new_id("jobs-test"));
        fs::create_dir(&root).unwrap();
        Self {
            store: root.join("jobs"),
            root,
            extension: None,
            ceiling: None,
        }
    }
    fn command(&self, record: bool) -> Command {
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .args(["--json", "--job-state-dir"])
            .arg(&self.store);
        if record {
            command.args(["--record", "always"]);
        }
        if let Some(ceiling) = self.ceiling {
            command.arg("--job-max-runtime-ms").arg(ceiling.to_string());
        }
        if let Some((store, policy)) = &self.extension {
            command
                .arg("--extension-state-dir")
                .arg(store)
                .arg("--extension-policy")
                .arg(policy);
        }
        command
    }
    fn run(&self, record: bool, args: &[&str]) -> Value {
        receipt(self.command(record).args(args).output().unwrap())
    }
    fn process(&self, id: &str, args: Vec<Value>) -> Request {
        let mut request = Request::new(
            "process.exec",
            json!({"program":RUNNER,"args":args,"cwd":self.root}),
        );
        request.request_id = id.into();
        request
    }
    fn delayed(&self, id: &str, delay_ms: u64) -> (Request, PathBuf, PathBuf) {
        let ready = self.root.join(format!("{id}-ready"));
        let marker = self.root.join(format!("{id}-finished"));
        (
            self.process(
                id,
                vec![
                    json!("ready-delay-mark"),
                    json!(ready),
                    json!(marker),
                    json!(delay_ms.to_string()),
                ],
            ),
            ready,
            marker,
        )
    }
    fn submit(&self, request: &Request, runtime: u64, retained: u64) -> Value {
        let path = self.root.join("request.json");
        fs::write(&path, serde_json::to_vec(request).unwrap()).unwrap();
        self.run(
            true,
            &[
                "job",
                "submit",
                "--request",
                path.to_str().unwrap(),
                "--max-runtime-ms",
                &runtime.to_string(),
                "--retain-events",
                &retained.to_string(),
            ],
        )
    }
    fn start(&self, id: &str) -> Worker {
        Worker(Some(
            self.command(true)
                .args(["job", "work", id])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ))
    }
    fn status(&self, id: &str) -> Value {
        self.run(false, &["job", "status", id])
    }
    fn install_capacity(&mut self) {
        let path = std::env::var_os("WORKBENCH_TEST_CAPACITY_PEER").expect(
            "Set WORKBENCH_TEST_CAPACITY_PEER to the explicitly built native capacity executable",
        );
        let executable = fs::read(path).unwrap();
        let mut manifest: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/extensions/manifest.json"
        ))
        .unwrap();
        manifest["id"] = json!("example.capacity");
        manifest["executable"]["sha256"] = json!(digest(&executable));
        manifest["executable"]["bytes"] = json!(executable.len());
        manifest["capabilities"][0]["operation"] = json!(CAPACITY);
        manifest["capabilities"][0]["output_schema"] = json!({
            "$schema":"https://json-schema.org/draft/2020-12/schema", "type":"object", "additionalProperties":false,
            "properties":{
                "headroom_bytes":{"type":"integer","minimum":0,"maximum":9007199254740991u64},
                "days_remaining_floor":{"type":"integer","minimum":0,"maximum":9007199254740991u64},
                "exhausted":{"type":"boolean"},"has_finite_horizon":{"type":"boolean"},
                "projection_state":{"type":"string","minLength":1,"maxLength":32},
                "assumption":{"type":"string","minLength":1,"maxLength":128}
            },
            "required":["headroom_bytes","days_remaining_floor","exhausted","has_finite_horizon","projection_state","assumption"]
        });
        let package = serde_json::to_vec(&json!({"package_format":1,"manifest":manifest,"executable_base64":STANDARD.encode(executable)})).unwrap();
        let pin = digest(&package);
        let path = self.root.join("package.json");
        fs::write(&path, package).unwrap();
        let store = self.root.join("extensions");
        let installed = receipt(
            Command::new(SAVE)
                .env_clear()
                .args(["--json", "--extension-state-dir"])
                .arg(&store)
                .args(["extension", "install"])
                .arg(path)
                .args(["--expected-package-sha256", &pin])
                .output()
                .unwrap(),
        );
        assert_eq!(installed["execution"]["status"], "succeeded", "{installed}");
        let policy = self.root.join("extension-policy.json");
        fs::write(&policy, serde_json::to_vec(&json!({"policy_version":1,"execution_profile":"operator-reviewed-v1","packages":[{"package_sha256":pin,"capabilities":[CAPACITY]}]})).unwrap()).unwrap();
        fs::set_permissions(&policy, fs::Permissions::from_mode(0o600)).unwrap();
        self.extension = Some((store, policy));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn job_id(value: &Value) -> String {
    value["data"]["job"]["job_id"]
        .as_str()
        .unwrap_or_else(|| panic!("missing job: {value}"))
        .into()
}
fn state(value: &Value, expected: &str) {
    assert_eq!(value["data"]["job"]["state"], expected, "{value}");
}
fn terminal(value: &Value, expected: &str) {
    state(value, "terminal");
    assert_eq!(
        value["data"]["result"]["execution"]["status"], expected,
        "{value}"
    );
}

#[test]
fn duplicate_admission_and_terminal_delivery_never_dispatch_twice_and_watch_discloses_gaps() {
    let f = Fixture::new();
    let marker = f.root.join("dispatch-marker");
    let request = f.process("same-request", vec![json!("mark"), json!(marker)]);
    let admitted = f.submit(&request, 600_000, 2);
    state(&admitted, "queued");
    let id = job_id(&admitted);
    let duplicate = f.submit(&request, 600_000, 2);
    assert_eq!(job_id(&duplicate), id);
    assert_eq!(
        duplicate["data"]["job"]["request_sha256"],
        admitted["data"]["job"]["request_sha256"]
    );
    assert!(!marker.exists());
    let completed = receipt(f.start(&id).wait());
    terminal(&completed, "succeeded");
    assert_eq!(fs::read(&marker).unwrap(), b"executed");
    assert!(
        completed["data"]["result"]["effective_limits"]["timeout_ms"]
            .as_u64()
            .unwrap()
            > 300_000
    );
    let original_result = completed["data"]["result"].clone();
    fs::write(&marker, b"no second dispatch").unwrap();
    terminal(&f.run(true, &["job", "work", &id]), "succeeded");
    let stored = f.status(&id);
    assert_eq!(stored["data"]["result"], original_result);
    assert_eq!(fs::read(&marker).unwrap(), b"no second dispatch");
    let watched = f.run(false, &["job", "watch", &id, "--after-sequence", "0"]);
    assert!(watched["data"]["cursor"]["gap"].is_object(), "{watched}");
    let events = watched["data"]["events"].as_array().unwrap();
    assert!(!events.is_empty() && events.len() <= 2);
    for pair in events.windows(2) {
        assert!(pair[0]["sequence"].as_u64().unwrap() < pair[1]["sequence"].as_u64().unwrap());
    }
    let cursor = watched["data"]["cursor"]["last_sequence"].as_u64().unwrap();
    let reconnect = f.run(
        false,
        &["job", "watch", &id, "--after-sequence", &cursor.to_string()],
    );
    assert!(reconnect["data"]["events"].as_array().unwrap().is_empty());
    assert_eq!(reconnect["data"]["result"], original_result);
    let mut changed = request.clone();
    changed.inputs["args"] = json!(["exit", "7"]);
    let conflict = f.submit(&changed, 600_000, 2);
    assert_eq!(conflict["execution"]["status"], "denied");
    error(&conflict, "job_request_conflict");
    assert_eq!(f.status(&id)["data"]["result"], original_result);
}

#[test]
fn queued_and_active_cancellation_require_observed_terminal_outcomes() {
    let f = Fixture::new();
    let marker = f.root.join("never-dispatched");
    let request = f.process("cancel-queued", vec![json!("mark"), json!(marker)]);
    let id = job_id(&f.submit(&request, 10_000, 32));
    let cancelled = f.run(true, &["job", "cancel", &id]);
    terminal(&cancelled, "cancelled");
    terminal(&f.run(true, &["job", "work", &id]), "cancelled");
    assert!(!marker.exists());

    let (request, ready, marker) = f.delayed("cancel-active", 10_000);
    let id = job_id(&f.submit(&request, 20_000, 32));
    let worker = f.start(&id);
    wait_path(&ready);
    state(&f.status(&id), "running");
    let cancellation = f.run(true, &["job", "cancel", &id]);
    assert_eq!(cancellation["data"]["job"]["cancel_requested"], true);
    let completed = receipt(worker.wait());
    terminal(&completed, "cancelled");
    assert_eq!(
        completed["data"]["result"]["data"]["cleanup"]["confirmed"],
        true
    );
    assert!(!marker.exists());
    let before = f.status(&id)["data"]["result"].clone();
    f.run(true, &["job", "cancel", &id]);
    assert_eq!(f.status(&id)["data"]["result"], before);
}

#[test]
fn concurrent_worker_refuses_without_redispatch_or_blocking_status_and_cancellation() {
    let f = Fixture::new();
    let (request, ready, marker) = f.delayed("worker-ownership", 10_000);
    let id = job_id(&f.submit(&request, 20_000, 32));
    let first = f.start(&id);
    wait_path(&ready);
    let first_pid = fs::read(&ready).unwrap();
    let second_output = f.start(&id).wait();
    assert_eq!(second_output.status.code(), Some(2));
    let refused = receipt(second_output);
    assert_eq!(refused["execution"]["status"], "denied");
    error(&refused, "job_worker_busy");
    assert_eq!(fs::read(&ready).unwrap(), first_pid);
    assert!(!marker.exists());
    state(&f.status(&id), "running");
    let cancelled = f.run(true, &["job", "cancel", &id]);
    assert_eq!(cancelled["data"]["job"]["cancel_requested"], true);
    terminal(&receipt(first.wait()), "cancelled");
    assert!(!marker.exists());
}

#[test]
fn noisy_child_results_remain_bounded_truthful_and_identical_after_lookup() {
    let f = Fixture::new();
    for mode in ["noise", "controls"] {
        let request = f.process(
            &format!("bounded-{mode}"),
            vec![json!(mode), json!("1200000")],
        );
        let id = job_id(&f.submit(&request, 10_000, 32));
        let output = f.start(&id).wait();
        assert_eq!(output.status.code(), Some(0));
        let completed = receipt(output);
        terminal(&completed, "partial");
        let child = &completed["data"]["result"];
        assert_eq!(child["execution"]["child_exit_code"], 0);
        assert_eq!(child["output"]["stdout_truncated"], true);
        assert_eq!(child["output"]["stderr_truncated"], true);
        assert_eq!(child["effect_outcome"], "unknown");
        assert_eq!(child["data"]["cleanup"]["confirmed"], true);
        let bytes = fs::read(f.store.join(&id).join("result.json")).unwrap();
        assert!(bytes.len() <= 1024 * 1024);
        let stored_bytes: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(&stored_bytes, child);
        assert_eq!(completed["data"]["result_sha256"], digest(&bytes));
        let lookup = f.status(&id);
        terminal(&lookup, "partial");
        assert_eq!(&lookup["data"]["result"], child);
        assert_eq!(
            lookup["data"]["result_sha256"],
            completed["data"]["result_sha256"]
        );
        assert_eq!(
            fs::read(f.store.join(&id).join("result.json")).unwrap(),
            bytes
        );
    }
}

#[test]
fn worker_stdout_disconnect_keeps_execution_and_committed_result_available() {
    let f = Fixture::new();
    let (request, ready, marker) = f.delayed("lost-output", 1_500);
    let id = job_id(&f.submit(&request, 10_000, 32));
    let mut worker = f.start(&id);
    wait_path(&ready);
    assert!(
        !marker.exists(),
        "fixture must still be executing at disconnection"
    );
    worker.disconnect_stdout();
    let _delivery_failure = worker.wait();
    wait_path(&marker);
    let stored = f.status(&id);
    terminal(&stored, "succeeded");
    assert_eq!(stored["data"]["result"]["execution"]["child_exit_code"], 0);
    assert_eq!(fs::read(&marker).unwrap(), b"executed");
}

#[test]
fn killed_worker_requires_reconciliation_even_after_child_finishes_without_replay() {
    let f = Fixture::new();
    let (request, ready, marker) = f.delayed("worker-crash", 5_000);
    let id = job_id(&f.submit(&request, 10_000, 32));
    let mut worker = f.start(&id);
    wait_path(&ready);
    assert!(
        !marker.exists(),
        "fixture must still be executing at worker crash"
    );
    worker.kill();
    let killed = worker.wait();
    assert!(!killed.status.success());
    // This bounded child finishes naturally. No persisted PID is trusted for a kill.
    wait_path(&marker);
    let interrupted = f.status(&id);
    state(&interrupted, "recovery_required");
    assert!(interrupted["data"]["result"].is_null());
    assert_eq!(interrupted["data"]["job"]["effect_outcome"], "unknown");
    fs::write(&marker, b"never replay an uncertain dispatch").unwrap();
    let reconciled = f.run(true, &["job", "work", &id]);
    state(&reconciled, "recovery_required");
    assert_eq!(reconciled["execution"]["status"], "unknown");
    error(&reconciled, "job_recovery_required");
    assert_eq!(
        fs::read(&marker).unwrap(),
        b"never replay an uncertain dispatch"
    );
    assert_eq!(job_id(&f.submit(&request, 10_000, 32)), id);
}

#[test]
fn explicit_job_runtime_ceiling_does_not_widen_foreground_request_admission() {
    let mut f = Fixture::new();
    let marker = f.root.join("runtime-refusal");
    let mut request = f.process("ceiling", vec![json!("mark"), json!(marker)]);
    f.ceiling = Some(500);
    let refused = f.submit(&request, 501, 32);
    assert_ne!(refused["execution"]["status"], "succeeded");
    assert!(!marker.exists());
    request.limits.timeout_ms = 300_001;
    let path = f.root.join("foreground.json");
    fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
    let foreground = f.run(false, &["call", "--request", path.to_str().unwrap()]);
    assert_eq!(foreground["execution"]["status"], "denied");
    error(&foreground, "invalid_request");
    assert!(foreground["execution"]["child_exit_code"].is_null());
    assert!(!marker.exists());
}

#[test]
#[ignore = "Requires WORKBENCH_TEST_CAPACITY_PEER naming the explicitly built native extension"]
fn native_extension_jobs_bind_exact_authority_and_accept_one_hour_runtime_metadata() {
    let mut f = Fixture::new();
    f.install_capacity();
    let mut request = Request::new(
        CAPACITY,
        json!({"capacity_bytes":1000,"used_bytes":500,"growth_bytes_per_day":100}),
    );
    request.request_id = "extension-job".into();
    let id = job_id(&f.submit(&request, 3_600_000, 32));
    let completed = receipt(f.start(&id).wait());
    terminal(&completed, "succeeded");
    let projection = &completed["data"]["result"]["data"]["terminal"]["data"];
    assert_eq!(projection["headroom_bytes"], 500);
    assert_eq!(projection["days_remaining_floor"], 5);
    assert!(
        completed["data"]["result"]["effective_limits"]["timeout_ms"]
            .as_u64()
            .unwrap()
            > 300_000
    );
    request.request_id = "extension-authority-changed".into();
    let changed_id = job_id(&f.submit(&request, 3_600_000, 32));
    let policy = &f.extension.as_ref().unwrap().1;
    let mut changed: Value = serde_json::from_slice(&fs::read(policy).unwrap()).unwrap();
    changed["packages"] = json!([]);
    fs::write(policy, serde_json::to_vec(&changed).unwrap()).unwrap();
    let refused = f.run(true, &["job", "work", &changed_id]);
    terminal(&refused, "denied");
    error(&refused["data"]["result"], "job_binding_changed");
    assert!(refused["data"]["result"]["execution"]["child_exit_code"].is_null());
    assert!(refused["data"]["result"]["data"]["supervisor"].is_null());
    assert_eq!(refused["data"]["job"]["effect_outcome"], "not_attempted");
}
