#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
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
    time::{Duration, Instant},
};
const SAVE: &str = env!("CARGO_BIN_EXE_save");
fn digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}
fn private(path: &Path, value: &[u8]) {
    fs::write(path, value).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn receipt(output: Output, exit: i32) -> Value {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.len() <= 2 * 1024 * 1024);
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stdout)));
    assert_eq!(output.status.code(), Some(exit), "{value}");
    assert_eq!(value["assessment"], "not_assessed");
    value
}
struct Server {
    child: Child,
    readers: Vec<JoinHandle<Vec<u8>>>,
    endpoint: SocketAddr,
}
impl Server {
    fn start(path: &Path, id: &str) -> Self {
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
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        let stdout = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut ready = Vec::new();
            (&mut reader)
                .take(65536)
                .read_until(b'\n', &mut ready)
                .unwrap();
            let _ = sender.send(ready);
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
            readers: vec![stdout, stderr],
            endpoint: "127.0.0.1:0".parse().unwrap(),
        };
        let bytes = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("bounded runner readiness");
        let ready: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&bytes)));
        assert_eq!(ready["runner_id"], id);
        server.endpoint = ready["address"].as_str().unwrap().parse().unwrap();
        server
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            // SAFETY: this retained Child is the exact test server. Always resume
            // it before cleanup, including unwinding a failed interruption check.
            unsafe { libc::kill(self.child.id() as i32, libc::SIGCONT) };
            // SAFETY: Child owns the live process identity created by this fixture.
            unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
            let deadline = Instant::now() + Duration::from_secs(5);
            while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if self.child.try_wait().ok().flatten().is_none() {
                let _ = self.child.kill();
            }
            let _ = self.child.wait();
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}
struct Node {
    root: PathBuf,
    id: String,
    config: Value,
    client: Value,
    server: Option<Server>,
}
impl Node {
    fn new(root: PathBuf, id: &str, runtime: u64, binding: &str) -> Self {
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca = CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap();
        private(&root.join("ca.der"), ca.der());
        let mut pins = vec![];
        for (name, usage) in [
            ("server", ExtendedKeyUsagePurpose::ServerAuth),
            ("client", ExtendedKeyUsagePurpose::ClientAuth),
        ] {
            let mut params = CertificateParams::new(vec!["runner.test".into()]).unwrap();
            params.extended_key_usages = vec![usage];
            let key = KeyPair::generate().unwrap();
            let cert = params.signed_by(&key, &ca).unwrap();
            private(&root.join(format!("{name}.der")), cert.der());
            private(&root.join(format!("{name}.key")), &key.serialize_der());
            pins.push(digest(cert.der()));
        }
        let config = json!({"config_version":1,"runner_id":id,"listen":"127.0.0.1:0","state_dir":root.join("runner-state"),
            "ca_certificate":root.join("ca.der"),"certificate":root.join("server.der"),"private_key":root.join("server.key"),
            "principals":[{"id":"fleet.operator","certificate_sha256":pins[1]}],
            "targets":[{"id":"observed.target","snapshot":{"name":id,"environment":"isolated-test"}}],
            "grants":[{"principal":"fleet.operator","target":"observed.target","operation":"task.run","operation_version":1,
                "task":{"id":"error-budget","version":1,"binding_sha256":binding},"max_runtime_ms":runtime}]});
        let client = json!({"config_version":1,"runner_id":id,"endpoint":"127.0.0.1:0","server_name":"runner.test","server_certificate_sha256":pins[0],
            "ca_certificate":root.join("ca.der"),"certificate":root.join("client.der"),"private_key":root.join("client.key")});
        let mut node = Self {
            root,
            id: id.into(),
            config,
            client,
            server: None,
        };
        node.start();
        node
    }
    fn start(&mut self) {
        let path = self.root.join("server.json");
        private(&path, &serde_json::to_vec(&self.config).unwrap());
        let server = Server::start(&path, &self.id);
        self.client["endpoint"] = json!(server.endpoint.to_string());
        private(
            &self.root.join("client.json"),
            &serde_json::to_vec(&self.client).unwrap(),
        );
        self.server = Some(server);
    }
    fn job_ids(&self) -> Vec<String> {
        let owner = digest(b"fleet.operator");
        let jobs = self
            .root
            .join("runner-state")
            .join(format!("principal-{}", &owner[7..]))
            .join("jobs");
        let mut ids: Vec<_> = fs::read_dir(&jobs)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| {
                let path = jobs.join(name);
                name.starts_with("job-")
                    && path.join("request.json").is_file()
                    && path.join("lease").is_file()
                    && fs::read(path.join("state.json")).is_ok_and(|bytes| {
                        serde_json::from_slice::<Value>(&bytes).is_ok_and(|value| value.is_object())
                    })
            })
            .collect();
        ids.sort();
        ids
    }
    fn signal(&self, signal: i32) {
        let server = self.server.as_ref().unwrap();
        // SAFETY: the identity comes from this fixture's retained live Child,
        // never from an on-disk job PID or a remote response.
        assert_eq!(unsafe { libc::kill(server.child.id() as i32, signal) }, 0);
    }
    fn inventory(&self, id: &str) -> Value {
        json!({"id":id,"runner_id":self.id,"target":"observed.target","runner_config":self.root.join("client.json"),"principal":"fleet.operator"})
    }
}
struct Controller {
    child: Child,
    stdout: PathBuf,
    stderr: PathBuf,
}
impl Controller {
    fn finish(&mut self, exit: Option<i32>) -> Output {
        let deadline = Instant::now() + Duration::from_secs(12);
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "fleet controller exceeded bounded completion allowance"
            );
            thread::sleep(Duration::from_millis(10));
        };
        if let Some(exit) = exit {
            assert_eq!(status.code(), Some(exit));
        }
        Output {
            status,
            stdout: fs::read(&self.stdout).unwrap(),
            stderr: fs::read(&self.stderr).unwrap(),
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
struct Fixture {
    root: PathBuf,
    nodes: Vec<Node>,
    inventory: Value,
}
impl Fixture {
    fn new(runtimes: &[u64]) -> Self {
        let root = std::env::temp_dir().join(workbench_core::result::new_id("fleet-test"));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let description = receipt(
            Command::new(SAVE)
                .env_clear()
                .args(["--json", "task", "describe", "error-budget"])
                .output()
                .unwrap(),
            0,
        );
        let binding = description["data"]["source"]["binding_digest"]
            .as_str()
            .unwrap();
        let nodes: Vec<_> = runtimes
            .iter()
            .enumerate()
            .map(|(index, runtime)| {
                Node::new(
                    root.join(format!("node-{index}")),
                    &format!("runner.{index}"),
                    *runtime,
                    binding,
                )
            })
            .collect();
        let inventory = json!({"inventory_version":1,"revision":"fixture-v1","targets":nodes.iter().enumerate().map(|(index,node)|node.inventory(&format!("target.{index}"))).collect::<Vec<_>>()});
        let f = Self {
            root,
            nodes,
            inventory,
        };
        f.write_inventory();
        f
    }
    fn write_inventory(&self) {
        private(
            &self.root.join("inventory.json"),
            &serde_json::to_vec(&self.inventory).unwrap(),
        );
    }
    fn command(&self, record: bool) -> Command {
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .args(["--json", "--fleet-inventory"])
            .arg(self.root.join("inventory.json"))
            .arg("--fleet-state-dir")
            .arg(self.root.join("fleet-state"));
        if record {
            command.args(["--record", "always"]);
        }
        command
    }
    fn collect(
        &self,
        id: &str,
        targets: &[&str],
        concurrency: u64,
        threshold: u64,
        exit: i32,
    ) -> Value {
        let input = json!({"request_id":id,"targets":targets,"task":{"id":"error-budget","version":1,"input":{"slo":99.9,"bad_minutes":40.32}},"concurrency":concurrency,"failure_threshold":threshold});
        let path = self.root.join("input.json");
        private(&path, &serde_json::to_vec(&input).unwrap());
        receipt(
            self.command(true)
                .args(["fleet", "collect", "--input"])
                .arg(path)
                .output()
                .unwrap(),
            exit,
        )
    }
    fn start_collect(&self, id: &str, concurrency: u64, timeout_ms: u64) -> Controller {
        let path = self.root.join("controller-input.json");
        private(&path, &serde_json::to_vec(&json!({"request_id":id,"targets":["target.0","target.1"],"task":{"id":"error-budget","version":1,"input":{"slo":99.9,"bad_minutes":40.32}},"concurrency":concurrency,"failure_threshold":2})).unwrap());
        let stdout = self.root.join("controller.stdout");
        let stderr = self.root.join("controller.stderr");
        let child = self
            .command(true)
            .args(["fleet", "collect", "--input"])
            .arg(path)
            .arg("--timeout-ms")
            .arg(timeout_ms.to_string())
            .stdout(fs::File::create(&stdout).unwrap())
            .stderr(fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        Controller {
            child,
            stdout,
            stderr,
        }
    }
    fn wait_first_admission_and_pause(&self, controller: &mut Controller) -> Vec<Vec<String>> {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            assert!(
                controller.child.try_wait().unwrap().is_none(),
                "controller completed before interruption boundary"
            );
            if self.nodes.iter().any(|node| !node.job_ids().is_empty()) {
                for node in &self.nodes {
                    node.signal(libc::SIGSTOP);
                }
                let jobs: Vec<_> = self.nodes.iter().map(Node::job_ids).collect();
                assert_eq!(
                    jobs.iter().map(Vec::len).sum::<usize>(),
                    1,
                    "single-concurrency fixture must pause after the first admission"
                );
                assert!(controller.child.try_wait().unwrap().is_none());
                return jobs;
            }
            assert!(Instant::now() < deadline, "no remote admission observed");
            thread::sleep(Duration::from_millis(1));
        }
    }
    fn read(&self, id: &str) -> Value {
        receipt(
            self.command(false)
                .args(["fleet", "status", id])
                .output()
                .unwrap(),
            0,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.nodes.clear();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn empty_duplicate_and_unknown_target_selections_are_refused() {
    let f = Fixture::new(&[30_000, 30_000]);
    for (id, targets) in [
        ("empty-selection", vec![]),
        ("duplicate-selection", vec!["target.0", "target.0"]),
        ("unknown-selection", vec!["target.absent"]),
    ] {
        let refused = f.collect(id, &targets, 1, 1, 2);
        assert_eq!(refused["execution"]["status"], "denied");
        assert_eq!(refused["effect_outcome"], "not_attempted");
    }
}

#[test]
fn frozen_selection_deduplicates_and_survives_current_inventory_changes() {
    let mut f = Fixture::new(&[30_000, 30_000]);
    let complete = f.collect("frozen-once", &["target.0", "target.1"], 2, 2, 0);
    assert_eq!(complete["data"]["aggregate"], "succeeded");
    assert_eq!(
        complete["data"]["counts"],
        json!({"succeeded":2,"failed":0,"unknown":0,"skipped":0,"pending":0})
    );
    assert_eq!(complete["data"]["targets"].as_array().unwrap().len(), 2);
    for target in complete["data"]["targets"].as_array().unwrap() {
        assert_eq!(
            target["receipt"]["data"]["result"]["execution"]["status"],
            "succeeded"
        );
        assert_eq!(
            target["receipt"]["data"]["result"]["data"]["calculation"]["status"]["remaining"]
                .as_f64(),
            Some(0.0)
        );
    }
    let limited = receipt(
        f.command(false)
            .args([
                "fleet",
                "status",
                "frozen-once",
                "--max-output-bytes",
                "1024",
            ])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(limited["execution"]["status"], "partial");
    assert_eq!(limited["data"]["counts"], complete["data"]["counts"]);
    assert_eq!(limited["data"]["targets"].as_array().unwrap().len(), 2);
    for (bounded, original) in limited["data"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .zip(complete["data"]["targets"].as_array().unwrap())
    {
        for field in [
            "id",
            "request_id",
            "outcome",
            "result_sha256",
            "evidence_ref",
        ] {
            assert_eq!(bounded[field], original[field]);
        }
        assert!(bounded["receipt"].is_null());
        assert_eq!(bounded["receipt_omitted"], true);
    }
    let id = complete["data"]["run"]["id"].as_str().unwrap();
    let jobs: Vec<_> = f.nodes.iter().map(Node::job_ids).collect();
    assert!(jobs.iter().all(|ids| ids.len() == 1));
    let duplicate = f.collect("frozen-once", &["target.0", "target.1"], 2, 2, 0);
    assert_eq!(duplicate["data"]["run"]["id"], id);
    assert_eq!(f.nodes.iter().map(Node::job_ids).collect::<Vec<_>>(), jobs);
    let conflict = f.collect("frozen-once", &["target.0"], 2, 2, 2);
    assert_eq!(conflict["execution"]["status"], "denied");
    assert!(
        conflict["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == "fleet_request_conflict")
    );
    f.inventory["revision"] = json!("changed-after-collection");
    f.inventory["targets"] = json!([]);
    f.write_inventory();
    let observed = f.read("frozen-once");
    assert_eq!(observed["data"]["run"]["id"], id);
    assert_eq!(observed["data"]["targets"], complete["data"]["targets"]);
    assert_eq!(observed["data"]["counts"], complete["data"]["counts"]);
    assert_eq!(f.nodes.iter().map(Node::job_ids).collect::<Vec<_>>(), jobs);
}

#[test]
fn remote_job_timeout_is_a_visible_failure_alongside_success() {
    let f = Fixture::new(&[30_000, 1]);
    let mixed = f.collect("mixed-child-results", &["target.0", "target.1"], 2, 2, 1);
    assert_eq!(mixed["execution"]["status"], "partial");
    assert_eq!(mixed["data"]["aggregate"], "partial");
    assert_eq!(
        mixed["data"]["counts"],
        json!({"succeeded":1,"failed":1,"unknown":0,"skipped":0,"pending":0})
    );
    assert_eq!(mixed["data"]["targets"].as_array().unwrap().len(), 2);
    assert_eq!(
        f.read("mixed-child-results")["data"]["counts"],
        mixed["data"]["counts"]
    );
    assert_eq!(f.nodes[0].job_ids().len(), 1);
    assert_eq!(f.nodes[1].job_ids().len(), 1);
    let failed = mixed["data"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["outcome"] == "failed")
        .unwrap();
    assert_eq!(
        failed["receipt"]["data"]["result"]["execution"]["status"],
        "timed_out"
    );
}

#[test]
fn unreachable_selected_runner_remains_visible_and_does_not_hide_success() {
    let mut f = Fixture::new(&[30_000, 30_000]);
    drop(f.nodes[1].server.take());
    let mixed = f.collect("unreachable-selected", &["target.0", "target.1"], 2, 2, 1);
    assert_eq!(mixed["data"]["aggregate"], "partial");
    assert_eq!(mixed["data"]["counts"]["succeeded"], 1);
    assert_eq!(mixed["data"]["counts"]["failed"], 1);
    assert_eq!(mixed["data"]["targets"].as_array().unwrap().len(), 2);
    assert_eq!(f.nodes[0].job_ids().len(), 1);
    assert!(f.nodes[1].job_ids().is_empty());
}

#[test]
fn failure_threshold_stops_future_admission_and_persisted_rotation_changes_first_target() {
    let f = Fixture::new(&[1, 1]);
    let first = f.collect("rotation-first", &["target.0", "target.1"], 1, 1, 1);
    assert_eq!(first["data"]["aggregate"], "failed");
    assert_eq!(
        first["data"]["counts"],
        json!({"succeeded":0,"failed":1,"unknown":0,"skipped":1,"pending":0})
    );
    let first_counts: Vec<_> = f.nodes.iter().map(|node| node.job_ids().len()).collect();
    assert_eq!(first_counts.iter().sum::<usize>(), 1);
    let second = f.collect("rotation-second", &["target.0", "target.1"], 1, 1, 1);
    assert_eq!(second["data"]["counts"], first["data"]["counts"]);
    assert_eq!(
        f.nodes
            .iter()
            .map(|node| node.job_ids().len())
            .collect::<Vec<_>>(),
        vec![1, 1]
    );
}

#[test]
fn interrupted_controller_preserves_ids_and_profile_drift_cannot_authorize_resubmission() {
    let f = Fixture::new(&[30_000, 30_000]);
    let mut controller = f.start_collect("interrupted-parent", 1, 30_000);
    let jobs = f.wait_first_admission_and_pause(&mut controller);
    controller.child.kill().unwrap();
    let killed = controller.finish(None);
    assert!(!killed.status.success());
    for node in &f.nodes {
        node.signal(libc::SIGCONT);
    }
    let interrupted = f.read("interrupted-parent");
    assert_eq!(interrupted["data"]["aggregate"], "unknown");
    let run_id = interrupted["data"]["run"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let dispatched = jobs.iter().position(|ids| !ids.is_empty()).unwrap();
    let profile = f.nodes[dispatched].root.join("client.json");
    let original = fs::read(&profile).unwrap();
    let mut changed = original.clone();
    changed.push(b'\n');
    private(&profile, &changed);
    let refused = receipt(
        f.command(true)
            .args(["fleet", "resume", "interrupted-parent"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(refused["data"]["aggregate"], "unknown");
    assert_eq!(refused["data"]["run"]["id"], run_id);
    assert_eq!(f.nodes.iter().map(Node::job_ids).collect::<Vec<_>>(), jobs);
    private(&profile, &original);
    let reconciled = receipt(
        f.command(true)
            .args(["fleet", "resume", "interrupted-parent"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(reconciled["data"]["run"]["id"], run_id);
    assert_eq!(reconciled["data"]["counts"]["skipped"], 1);
    assert_eq!(reconciled["data"]["counts"]["unknown"], 0);
    assert_eq!(f.nodes.iter().map(Node::job_ids).collect::<Vec<_>>(), jobs);
}

#[test]
fn active_cancellation_keeps_all_selected_targets_visible() {
    let f = Fixture::new(&[30_000, 30_000]);
    let mut controller = f.start_collect("cancel-parent", 1, 30_000);
    f.wait_first_admission_and_pause(&mut controller);
    let cancelled = receipt(
        f.command(true)
            .args(["fleet", "cancel", "cancel-parent"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(cancelled["data"]["targets"].as_array().unwrap().len(), 2);
    for node in &f.nodes {
        node.signal(libc::SIGCONT);
    }
    let completed = receipt(controller.finish(Some(1)), 1);
    assert_eq!(completed["data"]["targets"].as_array().unwrap().len(), 2);
    assert_eq!(completed["data"]["counts"]["skipped"], 1);
    assert_eq!(completed["data"]["counts"]["unknown"], 0);
    assert_ne!(completed["data"]["aggregate"], "succeeded");
}

#[test]
fn one_controller_deadline_keeps_the_complete_selected_set_visible() {
    let f = Fixture::new(&[30_000, 30_000]);
    f.nodes[1].signal(libc::SIGSTOP);
    let started = Instant::now();
    let mut controller = f.start_collect("global-deadline", 2, 300);
    let output = controller.finish(None);
    for node in &f.nodes {
        node.signal(libc::SIGCONT);
    }
    assert!(started.elapsed() < Duration::from_secs(4));
    let exit = output.status.code().unwrap();
    assert!(matches!(exit, 1 | 124));
    let bounded = receipt(output, exit);
    assert_eq!(bounded["data"]["targets"].as_array().unwrap().len(), 2);
    assert_ne!(bounded["data"]["aggregate"], "succeeded");
    let accounted: u64 = bounded["data"]["counts"]
        .as_object()
        .unwrap()
        .values()
        .map(|value| value.as_u64().unwrap())
        .sum();
    assert_eq!(accounted, 2);
}
