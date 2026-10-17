#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Server {
    child: Child,
    authority: String,
    token: String,
}
struct Reply {
    status: u16,
    headers: String,
    body: Vec<u8>,
}
impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("JSON response")
    }
}
impl Server {
    #[allow(dead_code)]
    fn start(grants: &[&str]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_save"));
        command.args(["ui", "serve"]);
        for grant in grants {
            command.args(["--allow", grant]);
        }
        Self::launch(command)
    }
    fn launch(mut command: Command) -> Self {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut link = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut link)
            .unwrap();
        let (origin, token) = link.trim().split_once("/#token=").expect("launch link");
        assert_eq!(token.len(), 43);
        let authority = origin.strip_prefix("http://").unwrap().to_owned();
        assert!(authority.starts_with("127.0.0.1:"));
        Self {
            child,
            authority,
            token: token.to_owned(),
        }
    }
    fn request(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        auth: bool,
        origin: bool,
        extra: &str,
    ) -> Reply {
        let mut headers = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n",
            self.authority,
            body.len()
        );
        if auth {
            headers.push_str(&format!("Authorization: Bearer {}\r\n", self.token));
        }
        if origin {
            headers.push_str(&format!("Origin: http://{}\r\n", self.authority));
        }
        headers.push_str(extra);
        headers.push_str("\r\n");
        self.raw(headers.as_bytes(), body)
    }
    fn raw(&self, headers: &[u8], body: &[u8]) -> Reply {
        let mut socket = TcpStream::connect(&self.authority).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(6)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket.write_all(headers).unwrap();
        // A size rejection can close before the remaining body is read.
        let _ = socket.write_all(body);
        let mut reply = Vec::new();
        socket
            .take(3 * 1024 * 1024)
            .read_to_end(&mut reply)
            .unwrap();
        let split = reply
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("HTTP header");
        let headers = String::from_utf8(reply[..split].to_vec()).unwrap();
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        Reply {
            status,
            headers,
            body: reply[split + 4..].to_vec(),
        }
    }
    fn get(&self, path: &str) -> Reply {
        self.request("GET", path, &[], true, false, "")
    }
    fn post(&self, path: &str, value: &Value) -> Reply {
        self.request(
            "POST",
            path,
            &serde_json::to_vec(value).unwrap(),
            true,
            true,
            "Content-Type: application/json\r\n",
        )
    }
    fn terminal(&self, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let result = self.get(&format!("/api/v1/runs/{id}")).json();
            if result["state"] == "terminal" {
                return result;
            }
            assert!(Instant::now() < deadline, "terminal receipt deadline");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        // SAFETY: child.id is this test's live process; signal has no borrowed pointers.
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline {
            if self.child.try_wait().unwrap().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        panic!("server failed its bounded shutdown");
    }
}

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use workbench_core::{EvidenceStoreOptions, OperatorContext, RunControl, request::Request};

const OPERATIONS: [&str; 5] = [
    "context.resolve",
    "evidence.list",
    "evidence.open",
    "evidence.compare",
    "report.create",
];
struct Fixture {
    directory: PathBuf,
    source: PathBuf,
    store: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(workbench_core::result::new_id("investigation-ui"));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let source = directory.join("context.json");
        let mut context: Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/context/unique.json"))
                .unwrap();
        context["records"][0]["last_reviewed"] = json!(workbench_core::result::timestamp());
        fs::write(&source, context.to_string()).unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
        let store = directory.join("store");
        copy_store(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/evidence/evidence-comparison-2026-10-10/installed-r2/store"),
            &store,
        );
        Self {
            directory,
            source,
            store,
        }
    }
    fn context(&self) -> OperatorContext {
        OperatorContext {
            context_source_path: Some(self.source.clone()),
            context_max_age_days: Some(30),
            evidence_store: Some(EvidenceStoreOptions::new(self.store.clone())),
            ..OperatorContext::default()
        }
    }
    fn launch(&self, operations: &[&str]) -> Server {
        let mut command = Command::new(env!("CARGO_BIN_EXE_save"));
        if operations.contains(&"context.resolve") {
            command
                .arg("--context-source")
                .arg(&self.source)
                .args(["--context-max-age-days", "30"]);
        }
        if operations.iter().any(|op| *op != "context.resolve") {
            command.arg("--state-dir").arg(&self.store);
        }
        command.args(["ui", "serve"]);
        for operation in operations {
            command.args(["--allow", operation]);
        }
        Server::launch(command)
    }
    fn bundles(&self) -> Vec<Value> {
        let mut entries = fs::read_dir(self.store.join("runs"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        entries.sort();
        entries.into_iter().map(|id| json!({"id":id})).collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}
fn copy_store(source: &Path, destination: &Path) {
    fs::create_dir(destination).unwrap();
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700)).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_store(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
            fs::set_permissions(target, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
}
fn submission(operation: &str, inputs: Value, index: usize) -> Value {
    json!({"submission_id":format!("12345678-1234-4234-8234-{index:012}"),"operation":operation,"operation_version":1,"inputs":inputs,"limits":{"timeout_ms":30000,"max_output_bytes":1048576}})
}
fn receipt(server: &Server, submission: &Value) -> Value {
    let admitted = server.post("/api/v1/runs", submission);
    assert_eq!(admitted.status, 202, "{}", admitted.json());
    assert!(
        admitted
            .headers
            .to_ascii_lowercase()
            .contains("location: /api/v1/runs/")
    );
    let terminal = server.terminal(admitted.json()["id"].as_str().unwrap());
    let downloaded = server.get(terminal["receipt_url"].as_str().unwrap());
    assert_eq!(downloaded.status, 200);
    let result = downloaded.json();
    assert_eq!(result["record_mode"], "never");
    assert_eq!(result["assessment"], "not_assessed");
    assert_eq!(server.post("/api/v1/runs", submission).json(), terminal);
    assert_eq!(
        server
            .get(&format!(
                "/api/v1/runs/{}",
                terminal["id"].as_str().unwrap()
            ))
            .json(),
        terminal
    );
    result
}
fn stable(mut value: Value) -> Value {
    // Core-generated IDs and clocks vary; historical evidence and all semantic fields remain exact.
    if let Some(object) = value.as_object_mut() {
        for field in [
            "request_id",
            "run_id",
            "started_at",
            "finished_at",
            "duration_ms",
        ] {
            object.remove(field);
        }
    }
    if value["operation"] == "evidence.open" {
        value["data"]["analysis"]
            .as_object_mut()
            .unwrap()
            .remove("at");
    }
    if value["operation"] == "report.create" {
        let generated = value["data"]["report"]["generated_at"]
            .as_str()
            .unwrap()
            .to_owned();
        let content = value["data"]["rendering"]["content"].as_str().unwrap();
        assert_eq!(
            content.len() as u64,
            value["data"]["rendering"]["bytes_required"]
                .as_u64()
                .unwrap()
        );
        assert!(content.contains(&generated));
        let content = content.replace(&generated, "GENERATED_TIME");
        value["data"]["rendering"]["bytes_required"] = json!(content.len());
        value["data"]["rendering"]["content"] = json!(content);
        value["data"]["report"]
            .as_object_mut()
            .unwrap()
            .remove("generated_at");
    }
    if value["operation"] == "context.resolve" {
        for source in value["sources"].as_array_mut().unwrap() {
            source.as_object_mut().unwrap().remove("observed_at");
        }
        value["data"]["source"]
            .as_object_mut()
            .unwrap()
            .remove("observed_at");
        value["data"]["resolution"]
            .as_object_mut()
            .unwrap()
            .remove("age_seconds");
    }
    value
}
#[test]
fn real_http_investigations_return_core_receipts_dedup_history_and_json() {
    let fixture = Fixture::new();
    let server = fixture.launch(&OPERATIONS);
    let session = server.get("/api/v1/session").json();
    assert_eq!(session["operations"].as_array().unwrap().len(), 5);
    assert_eq!(session["roots"], json!([]));
    assert_eq!(session["targets"], json!([]));
    for op in session["operations"].as_array().unwrap() {
        assert_eq!(
            op["profile"],
            if op["id"] == "context.resolve" {
                "offline-context-v1"
            } else {
                "verified-evidence-v1"
            }
        );
        assert_eq!(op.as_object().unwrap().len(), 3);
    }
    assert!(
        !session
            .to_string()
            .contains(fixture.directory.to_str().unwrap())
    );
    let bundles = fixture.bundles();
    assert!(bundles.len() >= 2);
    let inputs = [
        json!({"service":"checkout","environment":"dev"}),
        json!({}),
        bundles[0].clone(),
        json!({"before":bundles[0],"after":bundles[1],"normalization":"exact-window-v1"}),
        json!({"bundles":[bundles[0]],"format":"markdown"}),
    ];
    let context = fixture.context();
    let grants = OPERATIONS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let grant = workbench_core::investigation_grant(&context, &grants).unwrap();
    for (index, (operation, inputs)) in OPERATIONS.into_iter().zip(inputs).enumerate() {
        let payload = submission(operation, inputs.clone(), index);
        let actual = receipt(&server, &payload);
        if matches!(operation, "evidence.open" | "evidence.compare") {
            let mut explicit = payload.clone();
            explicit["limits"]["max_items"] = json!(100);
            assert_eq!(server.post("/api/v1/runs", &explicit).status, 202);
            explicit["limits"]["max_items"] = json!(1);
            assert_eq!(server.post("/api/v1/runs", &explicit).status, 409);
        }
        if operation == "report.create" {
            let mut explicit = payload.clone();
            explicit["inputs"]["title"] = json!("Evidence handoff");
            explicit["inputs"]["proposals"] = json!([]);
            assert_eq!(server.post("/api/v1/runs", &explicit).status, 202);
        }
        assert_eq!(
            actual["execution"]["status"],
            if operation == "evidence.compare" {
                "partial"
            } else {
                "succeeded"
            },
            "{actual}"
        );
        let expected = serde_json::to_value(workbench_core::execute_with_investigation_grant(
            Request::new(operation, inputs),
            &RunControl::default(),
            &grant,
        ))
        .unwrap();
        assert_eq!(stable(actual), stable(expected), "{operation}");
        let mut changed = payload.clone();
        changed["limits"]["timeout_ms"] = json!(29999);
        assert_eq!(server.post("/api/v1/runs", &changed).status, 409);
    }
    assert_eq!(
        server.get("/api/v1/runs").json()["runs"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
}
#[test]
fn browser_cannot_supply_selectors_paths_roles_or_ungranted_operations() {
    let fixture = Fixture::new();
    let server = fixture.launch(&OPERATIONS);
    for (index, operation) in OPERATIONS.iter().enumerate() {
        for (field, value) in [
            ("root_id", Value::Null),
            ("target_id", Value::Null),
            ("record", json!("always")),
            ("config", json!("/tmp/config")),
            ("role", json!("operator")),
        ] {
            let mut payload = submission(operation, json!({}), index);
            payload[field] = value;
            assert_eq!(
                server.post("/api/v1/runs", &payload).status,
                422,
                "{operation} {field}"
            );
        }
        let payload = submission(operation, json!({"path":"/tmp/arbitrary"}), index);
        assert_eq!(
            server.post("/api/v1/runs", &payload).status,
            422,
            "{operation}"
        );
    }
    for operation in [
        "evidence.collect",
        "evidence.replay",
        "evidence.delete",
        "extension.run",
        "change.apply",
        "command.inspect",
    ] {
        assert_eq!(
            server
                .post("/api/v1/runs", &submission(operation, json!({}), 90))
                .status,
            403
        );
    }
    for operation in ["context.resolve", "evidence.list", "report.create"] {
        let mut payload = submission(operation, json!({}), 91);
        payload["limits"]["max_items"] = json!(10);
        assert_eq!(server.post("/api/v1/runs", &payload).status, 422);
    }
    for operation in ["evidence.open", "evidence.compare"] {
        for value in [Value::Null, json!(0), json!(1001), json!(true)] {
            let mut payload = submission(operation, json!({}), 91);
            payload["limits"]["max_items"] = value;
            assert_eq!(server.post("/api/v1/runs", &payload).status, 422);
        }
    }
    assert_eq!(server.get("/api/v1/runs").json()["runs"], json!([]));
    let limited = fixture.launch(&["evidence.list"]);
    assert_eq!(
        limited.get("/api/v1/session").json()["operations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for operation in [
        "context.resolve",
        "evidence.open",
        "evidence.compare",
        "report.create",
    ] {
        assert_eq!(
            limited
                .post("/api/v1/runs", &submission(operation, json!({}), 0))
                .status,
            403
        );
    }
}
#[test]
fn pinned_source_and_store_drift_refuse_dispatch_with_canonical_receipts() {
    let fixture = Fixture::new();
    let server = fixture.launch(&OPERATIONS);
    let mut source: Value = serde_json::from_slice(&fs::read(&fixture.source).unwrap()).unwrap();
    source["source"]["revision"] = json!("changed");
    fs::write(&fixture.source, source.to_string()).unwrap();
    let denied = receipt(
        &server,
        &submission(
            "context.resolve",
            json!({"service":"checkout","environment":"dev"}),
            0,
        ),
    );
    assert_ne!(denied["execution"]["status"], "succeeded");
    assert!(!denied["errors"].as_array().unwrap().is_empty());
    let old = fixture.directory.join("old-store");
    fs::rename(&fixture.store, &old).unwrap();
    copy_store(&old, &fixture.store);
    let denied = receipt(&server, &submission("evidence.list", json!({}), 1));
    assert_ne!(denied["execution"]["status"], "succeeded");
    assert!(!denied["errors"].as_array().unwrap().is_empty());
}
#[test]
fn startup_requires_exact_grants_and_existing_sources_without_creation() {
    let fixture = Fixture::new();
    let missing = fixture.directory.join("must-not-be-created");
    let cases = vec![
        vec!["ui", "serve", "--allow", "context.resolve"],
        vec!["ui", "serve", "--allow", "evidence.list"],
        vec!["ui", "serve", "--allow", "evidence.open"],
        vec!["ui", "serve", "--allow", "evidence.compare"],
        vec!["ui", "serve", "--allow", "report.create"],
        vec![
            "--context-source",
            fixture.source.to_str().unwrap(),
            "--context-max-age-days",
            "30",
            "ui",
            "serve",
        ],
        vec![
            "--state-dir",
            fixture.store.to_str().unwrap(),
            "ui",
            "serve",
        ],
        vec![
            "--state-dir",
            missing.to_str().unwrap(),
            "ui",
            "serve",
            "--allow",
            "evidence.list",
        ],
        vec![
            "--context-source",
            missing.to_str().unwrap(),
            "--context-max-age-days",
            "30",
            "ui",
            "serve",
            "--allow",
            "context.resolve",
        ],
        vec![
            "--state-dir",
            fixture.store.to_str().unwrap(),
            "--store-max-bytes",
            "1048576",
            "ui",
            "serve",
            "--allow",
            "evidence.list",
        ],
        vec![
            "--state-dir",
            fixture.store.to_str().unwrap(),
            "--retention-days",
            "1",
            "ui",
            "serve",
            "--allow",
            "evidence.list",
        ],
        vec![
            "ui",
            "serve",
            "--allow",
            "evidence.list",
            "--allow",
            "evidence.list",
        ],
    ];
    for args in cases {
        let output = Command::new(env!("CARGO_BIN_EXE_save"))
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "startup exposed a launch URL");
    }
    assert!(!missing.exists());
}

#[test]
fn mixed_numerical_and_investigation_grants_dispatch_through_existing_worker() {
    let fixture = Fixture::new();
    let server = fixture.launch(&["evidence.list", "task.run"]);
    let operations = server.get("/api/v1/session").json()["operations"].clone();
    assert_eq!(operations.as_array().unwrap().len(), 2);
    let listed = receipt(&server, &submission("evidence.list", json!({}), 0));
    assert_eq!(listed["execution"]["status"], "succeeded");
    let calculated = receipt(
        &server,
        &submission(
            "task.run",
            json!({"id":"error-budget","version":1,"input":{"slo":99.9,"window_days":28,"bad_minutes":20}}),
            1,
        ),
    );
    assert_eq!(calculated["execution"]["status"], "succeeded");
}
