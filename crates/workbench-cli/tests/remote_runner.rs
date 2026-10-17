#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use rustls::{
    ClientConfig, ClientConnection, RootCertStore, StreamOwned,
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
const SAVE: &str = env!("CARGO_BIN_EXE_save");
const RUNNER: &str = "fixture.runner";
fn digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}
fn private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn receipt(output: Output, code: i32) -> Value {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.len() <= 2 * 1024 * 1024);
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stdout)));
    assert_eq!(output.status.code(), Some(code), "{value}");
    assert_eq!(value["assessment"], "not_assessed");
    value
}
fn has_error(value: &Value, code: &str) {
    assert!(
        value["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == code),
        "{value}"
    );
}
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
    fn stop(mut self) {
        // SAFETY: this PID belongs to the live Child retained by this test; it
        // comes from process creation, never from persisted runner metadata.
        assert_eq!(
            // SAFETY: this PID belongs to the live child owned by this test server.
            unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) },
            0
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "runner ignored bounded shutdown");
            thread::sleep(Duration::from_millis(10));
        };
        self.stopped = true;
        assert_eq!(status.code(), Some(143));
        assert!(self.stdout.take().unwrap().join().unwrap().is_empty());
        let stderr = self.stderr.take().unwrap().join().unwrap();
        assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
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
struct Fixture {
    root: PathBuf,
    server_config: PathBuf,
    server: Value,
    client: Value,
    ca: Vec<u8>,
    certificates: Vec<(Vec<u8>, Vec<u8>)>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(workbench_core::result::new_id("remote-runner-test"));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();
        private(&root.join("ca.der"), ca.der());
        let mut certificates = Vec::new();
        for (index, name) in [
            "runner.test",
            "principal-a.test",
            "principal-b.test",
            "unknown.test",
            "runner.test",
        ]
        .into_iter()
        .enumerate()
        {
            let key = KeyPair::generate().unwrap();
            let mut params = CertificateParams::new(vec![name.to_owned()]).unwrap();
            params.extended_key_usages = vec![if index == 0 || index == 4 {
                ExtendedKeyUsagePurpose::ServerAuth
            } else {
                ExtendedKeyUsagePurpose::ClientAuth
            }];
            if index == 4 {
                params.not_before = rcgen::date_time_ymd(2019, 1, 1);
                params.not_after = rcgen::date_time_ymd(2020, 1, 1);
            }
            let certificate = params.signed_by(&key, &ca).unwrap();
            private(&root.join(format!("{index}.der")), certificate.der());
            private(&root.join(format!("{index}.key")), &key.serialize_der());
            certificates.push((certificate.der().to_vec(), key.serialize_der()));
        }
        let describe = receipt(
            Command::new(SAVE)
                .env_clear()
                .args(["--json", "task", "describe", "error-budget"])
                .output()
                .unwrap(),
            0,
        );
        let binding = describe["data"]["source"]["binding_digest"]
            .as_str()
            .unwrap();
        let grant = |principal| json!({"principal":principal,"target":"fixture.target","operation":"task.run","operation_version":1,"task":{"id":"error-budget","version":1,"binding_sha256":binding},"max_runtime_ms":30000});
        let server = json!({"config_version":1,"runner_id":RUNNER,"listen":"127.0.0.1:0","state_dir":root.join("state"),"ca_certificate":root.join("ca.der"),"certificate":root.join("0.der"),"private_key":root.join("0.key"),"principals":[{"id":"principal.a","certificate_sha256":digest(&certificates[1].0)},{"id":"principal.b","certificate_sha256":digest(&certificates[2].0)}],"targets":[{"id":"fixture.target","snapshot":{"name":"Fixture target","environment":"isolated-test"}}],"grants":[grant("principal.a"),grant("principal.b")]});
        let client = json!({"config_version":1,"runner_id":RUNNER,"endpoint":"127.0.0.1:0","server_name":"runner.test","server_certificate_sha256":digest(&certificates[0].0),"ca_certificate":root.join("ca.der"),"certificate":root.join("1.der"),"private_key":root.join("1.key")});
        let server_config = root.join("server.json");
        private(&server_config, &serde_json::to_vec(&server).unwrap());
        Self {
            root,
            server_config,
            server,
            client,
            ca: ca.der().to_vec(),
            certificates,
        }
    }
    fn start(&self) -> Server {
        private(
            &self.server_config,
            &serde_json::to_vec(&self.server).unwrap(),
        );
        Server::start(&self.server_config)
    }
    fn client_config(&self, server: &Server, principal: usize) -> Value {
        let mut client = self.client.clone();
        client["endpoint"] = json!(server.endpoint.to_string());
        client["certificate"] = json!(self.root.join(format!("{principal}.der")));
        client["private_key"] = json!(self.root.join(format!("{principal}.key")));
        client
    }
    fn call_config(&self, config: &Value, args: &[&str], record: bool, code: i32) -> Value {
        let path = self.root.join("client.json");
        private(&path, &serde_json::to_vec(config).unwrap());
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .args(["--json", "--runner-config"])
            .arg(path);
        if record {
            command.args(["--record", "always"]);
        }
        receipt(command.args(args).output().unwrap(), code)
    }
    fn call(
        &self,
        server: &Server,
        principal: usize,
        args: &[&str],
        record: bool,
        code: i32,
    ) -> Value {
        self.call_config(&self.client_config(server, principal), args, record, code)
    }
    fn body(id: &str) -> Value {
        json!({"request_id":id,"target":"fixture.target","operation":"task.run","operation_version":1,"inputs":{"id":"error-budget","version":1,"input":{"slo":99.9,"bad_minutes":40.32}},"max_runtime_ms":30000,"retain_events":2})
    }
    fn submit(&self, server: &Server, principal: usize, body: &Value, code: i32) -> Value {
        let path = self.root.join("submission.json");
        private(&path, &serde_json::to_vec(body).unwrap());
        self.call(
            server,
            principal,
            &[
                "runner",
                "submit",
                RUNNER,
                "--input",
                path.to_str().unwrap(),
            ],
            true,
            code,
        )
    }
    fn terminal(&self, server: &Server, principal: usize, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = self.call(
                server,
                principal,
                &["runner", "status", RUNNER, id],
                false,
                0,
            );
            if status["data"]["job"]["state"] == "terminal" {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "job did not reach terminal: {status}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn raw(&self, server: &Server, principal: usize) -> StreamOwned<ClientConnection, TcpStream> {
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(self.ca.clone())).unwrap();
        let mut config = ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(roots)
        .with_client_auth_cert(
            vec![CertificateDer::from(self.certificates[principal].0.clone())],
            PrivatePkcs8KeyDer::from(self.certificates[principal].1.clone()).into(),
        )
        .unwrap();
        config.alpn_protocols = vec![b"save-runner/1".to_vec()];
        config.resumption = rustls::client::Resumption::disabled();
        config.enable_early_data = false;
        let mut connection = ClientConnection::new(
            Arc::new(config),
            ServerName::try_from("runner.test").unwrap(),
        )
        .unwrap();
        let mut socket =
            TcpStream::connect_timeout(&server.endpoint, Duration::from_secs(2)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        while connection.is_handshaking() {
            connection.complete_io(&mut socket).unwrap();
        }
        assert_eq!(
            connection.alpn_protocol(),
            Some(b"save-runner/1".as_slice())
        );
        assert_eq!(
            digest(connection.peer_certificates().unwrap()[0].as_ref()),
            self.client["server_certificate_sha256"]
        );
        StreamOwned::new(connection, socket)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn send_frame(stream: &mut impl Write, bytes: &[u8]) {
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(bytes).unwrap();
    stream.flush().unwrap();
}
fn receive_frame(stream: &mut impl Read) -> Value {
    let mut header = [0; 4];
    stream.read_exact(&mut header).unwrap();
    let length = u32::from_be_bytes(header) as usize;
    assert!(length <= 2 * 1024 * 1024);
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn principal_owned_requests_deduplicate_conflict_and_reconnect_without_cross_account_lookup() {
    let f = Fixture::new();
    let server = f.start();
    let handshake = f.call(&server, 1, &["runner", "handshake", RUNNER], false, 0);
    assert_eq!(handshake["data"]["principal"]["id"], "principal.a");
    assert_eq!(
        handshake["data"]["capabilities"][0]["operation"],
        "task.run"
    );
    let body = Fixture::body("shared-correlation");
    let admitted = f.submit(&server, 1, &body, 0);
    assert_eq!(admitted["data"]["admission"], "accepted");
    let id = admitted["data"]["job"]["job_id"].clone();
    let duplicate = f.submit(&server, 1, &body, 0);
    assert_eq!(duplicate["data"]["job"]["job_id"], id);
    let unknown_to_b = f.call(
        &server,
        2,
        &["runner", "status", RUNNER, "shared-correlation"],
        false,
        0,
    );
    assert_eq!(unknown_to_b["data"]["admission"], "not_found");
    assert!(unknown_to_b["data"]["result"].is_null());
    let b_job = f.submit(&server, 2, &body, 0);
    assert_ne!(b_job["data"]["job"]["job_id"], id);
    let terminal = f.terminal(&server, 1, "shared-correlation");
    assert_eq!(
        terminal["data"]["result"]["execution"]["status"],
        "succeeded"
    );
    assert_eq!(
        terminal["data"]["result"]["data"]["calculation"]["status"]["remaining"].as_f64(),
        Some(0.0)
    );
    let mut conflict = body;
    conflict["inputs"]["input"]["bad_minutes"] = json!(1);
    let refused = f.submit(&server, 1, &conflict, 2);
    has_error(&refused, "runner_remote_refusal");
    assert_eq!(
        f.terminal(&server, 1, "shared-correlation")["data"]["result"],
        terminal["data"]["result"]
    );
    let watch = f.call(
        &server,
        1,
        &[
            "runner",
            "watch",
            RUNNER,
            "shared-correlation",
            "--after-sequence",
            "0",
        ],
        false,
        0,
    );
    assert!(watch["data"]["cursor"]["gap"].is_object());
    let cursor = watch["data"]["cursor"]["next_sequence"].as_u64().unwrap();
    let reconnect = f.call(
        &server,
        1,
        &[
            "runner",
            "watch",
            RUNNER,
            "shared-correlation",
            "--after-sequence",
            &cursor.to_string(),
        ],
        false,
        0,
    );
    assert!(reconnect["data"]["events"].as_array().unwrap().is_empty());
    assert_eq!(reconnect["data"]["result"], terminal["data"]["result"]);
    f.terminal(&server, 2, "shared-correlation");
    server.stop();
}

#[test]
fn grant_revocation_keeps_owned_historical_result_observable_after_restart() {
    let mut f = Fixture::new();
    let server = f.start();
    f.submit(&server, 1, &Fixture::body("before-revocation"), 0);
    let before = f.terminal(&server, 1, "before-revocation");
    server.stop();
    f.server["grants"] = json!([]);
    let restarted = f.start();
    let after = f.call(
        &restarted,
        1,
        &["runner", "status", RUNNER, "before-revocation"],
        false,
        0,
    );
    assert_eq!(after["data"]["result"], before["data"]["result"]);
    assert_eq!(
        after["data"]["result_sha256"],
        before["data"]["result_sha256"]
    );
    let refused = f.submit(&restarted, 1, &Fixture::body("after-revocation"), 2);
    has_error(&refused, "runner_remote_refusal");
    assert_eq!(refused["data"]["remote_error"], "runner_grant_denied");
    restarted.stop();
}

#[test]
fn tls_identity_and_unknown_principal_refusals_leave_no_owned_admission() {
    let f = Fixture::new();
    let server = f.start();
    let good = f.client_config(&server, 1);
    for (field, value, code) in [
        ("server_name", json!("wrong.test"), "runner_tls_failed"),
        (
            "server_certificate_sha256",
            json!(format!("sha256:{}", "0".repeat(64))),
            "runner_server_identity_mismatch",
        ),
        (
            "ca_certificate",
            json!(f.root.join("missing.der")),
            "runner_config_unavailable",
        ),
    ] {
        let mut config = good.clone();
        config[field] = value;
        let refused = f.call_config(&config, &["runner", "handshake", RUNNER], false, 2);
        has_error(&refused, code);
        assert!(refused["data"]["job"].is_null());
    }
    let other_ca = CertifiedIssuer::self_signed(
        {
            let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
            params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
            params
        },
        KeyPair::generate().unwrap(),
    )
    .unwrap();
    let wrong_ca = f.root.join("wrong-ca.der");
    private(&wrong_ca, other_ca.der());
    let mut config = good;
    config["ca_certificate"] = json!(wrong_ca);
    has_error(
        &f.call_config(&config, &["runner", "handshake", RUNNER], false, 2),
        "runner_tls_failed",
    );
    let unknown = f.call(&server, 3, &["runner", "handshake", RUNNER], false, 2);
    assert_eq!(unknown["execution"]["status"], "denied");
    assert!(unknown["data"]["principal"].is_null());
    f.call(&server, 1, &["runner", "handshake", RUNNER], false, 0);
    server.stop();
}

#[test]
fn lost_ack_is_reconciled_by_status_without_resubmission_and_invalid_wire_never_dispatches() {
    let f = Fixture::new();
    let server = f.start();
    let mut stream = f.raw(&server, 1);
    send_frame(
        &mut stream,
        &serde_json::to_vec(
            &json!({"protocol":1,"method":"submit","inputs":Fixture::body("lost-ack")}),
        )
        .unwrap(),
    );
    // Send a TLS close notification without reading the admission response.
    stream.conn.send_close_notify();
    stream.flush().unwrap();
    drop(stream);
    let stored = f.terminal(&server, 1, "lost-ack");
    assert_eq!(stored["data"]["result"]["execution"]["status"], "succeeded");
    assert_eq!(stored["data"]["request_id"], "lost-ack");
    let mut stream = f.raw(&server, 1);
    send_frame(
        &mut stream,
        &serde_json::to_vec(
            &json!({"protocol":2,"method":"submit","inputs":Fixture::body("bad-protocol")}),
        )
        .unwrap(),
    );
    let refused = receive_frame(&mut stream);
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["error"], "runner_invalid_protocol");
    drop(stream);
    let missing = f.call(
        &server,
        1,
        &["runner", "status", RUNNER, "bad-protocol"],
        false,
        0,
    );
    assert_eq!(missing["data"]["admission"], "not_found");
    // An accepted TCP peer that never starts TLS must not delay server shutdown.
    let stalled = TcpStream::connect_timeout(&server.endpoint, Duration::from_secs(2)).unwrap();
    thread::sleep(Duration::from_millis(50));
    server.stop();
    drop(stalled);
}

#[test]
fn expired_server_certificate_refuses_even_with_matching_leaf_pin() {
    let mut f = Fixture::new();
    f.server["certificate"] = json!(f.root.join("4.der"));
    f.server["private_key"] = json!(f.root.join("4.key"));
    f.client["server_certificate_sha256"] = json!(digest(&f.certificates[4].0));
    let server = f.start();
    let refused = f.call(&server, 1, &["runner", "handshake", RUNNER], false, 2);
    has_error(&refused, "runner_tls_failed");
    assert!(refused["data"]["principal"].is_null());
    server.stop();
}

#[test]
fn server_bind_failure_discloses_retained_startup_writes() {
    let mut f = Fixture::new();
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    f.server["listen"] = json!(occupied.local_addr().unwrap().to_string());
    private(&f.server_config, &serde_json::to_vec(&f.server).unwrap());
    let failed = receipt(
        Command::new(SAVE)
            .env_clear()
            .args([
                "--json",
                "--record",
                "always",
                "runner",
                "serve",
                "--server-config",
            ])
            .arg(&f.server_config)
            .output()
            .unwrap(),
        1,
    );
    has_error(&failed, "runner_bind_failed");
    assert_eq!(failed["execution"]["status"], "unknown");
    assert_eq!(failed["effect_outcome"], "unknown");
    assert_eq!(failed["record_mode"], "always");
    assert!(f.root.join("state/format.json").is_file());
    assert!(fs::read_dir(f.root.join("state")).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("principal-")
    }));
    drop(occupied);
    // Retained initialization can be verified and reopened normally.
    f.server["listen"] = json!("127.0.0.1:0");
    let server = f.start();
    f.call(&server, 1, &["runner", "handshake", RUNNER], false, 0);
    server.stop();
}

#[test]
fn hosted_adapters_refuse_runner_credentials_before_startup() {
    for adapter in ["mcp", "ui"] {
        let refused = Command::new(SAVE)
            .env_clear()
            .args([
                "--runner-config",
                "/missing-private-runner-config.json",
                adapter,
                "serve",
            ])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(refused.status.code(), Some(2));
        assert!(
            refused.stdout.is_empty(),
            "{adapter} unexpectedly started: {}",
            String::from_utf8_lossy(&refused.stdout)
        );
        assert!(!refused.stderr.is_empty());
    }
}

#[test]
fn expected_principal_and_policy_refuse_before_remote_admission() {
    let f = Fixture::new();
    let server = f.start();
    for (request_id, expected) in [
        (
            "wrong-expected-principal",
            json!({"principal":"principal.b"}),
        ),
        (
            "wrong-expected-policy",
            json!({"principal":"principal.a","policy_sha256":format!("sha256:{}", "0".repeat(64))}),
        ),
    ] {
        let mut stream = f.raw(&server, 1);
        let wire = json!({"protocol":1,"method":"submit","inputs":Fixture::body(request_id),"expected":expected});
        send_frame(&mut stream, &serde_json::to_vec(&wire).unwrap());
        let refused = receive_frame(&mut stream);
        assert_eq!(refused["ok"], false);
        assert_eq!(refused["error"], "runner_precondition_failed");
        drop(stream);
        let absent = f.call(
            &server,
            1,
            &["runner", "status", RUNNER, request_id],
            false,
            0,
        );
        assert_eq!(absent["data"]["admission"], "not_found");
        assert!(absent["data"]["job"].is_null());
    }
    server.stop();
}
