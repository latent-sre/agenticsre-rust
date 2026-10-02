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
fn numerical() -> Value {
    json!({"submission_id":"12345678-1234-4234-8234-123456789abc","operation":"task.run","operation_version":1,"inputs":{"id":"error-budget","version":1,"input":{"slo":99.9,"window_days":28,"bad_minutes":20}},"limits":{"timeout_ms":30000,"max_output_bytes":1048576}})
}

#[test]
fn real_http_task_receipt_history_replay_and_clear() {
    let server = Server::start(&["task.run"]);
    let session = server.get("/api/v1/session");
    assert_eq!(session.status, 200);
    assert_eq!(
        session.json()["operations"],
        json!([{"id":"task.run","version":1,"tasks":["error-budget"]}])
    );
    let admitted = server.post("/api/v1/runs", &numerical());
    assert_eq!(admitted.status, 202);
    let id = admitted.json()["id"].as_str().unwrap().to_owned();
    assert!(
        admitted
            .headers
            .to_ascii_lowercase()
            .contains(&format!("location: /api/v1/runs/{id}"))
    );
    let terminal = server.terminal(&id);
    assert_eq!(terminal["execution_status"], "succeeded");
    let receipt = server.get(terminal["receipt_url"].as_str().unwrap());
    assert_eq!(receipt.status, 200);
    let receipt = receipt.json();
    assert_eq!(receipt["assessment"], "not_assessed");
    assert_eq!(receipt["record_mode"], "never");
    assert!(
        (receipt["data"]["calculation"]["status"]["budget"]
            .as_f64()
            .unwrap()
            - 40.32)
            .abs()
            < 1e-8
    );
    assert_eq!(
        server.get("/api/v1/runs?limit=1&offset=0").json()["runs"][0],
        terminal
    );
    assert_eq!(server.post("/api/v1/runs", &numerical()).json(), terminal);
    let mut conflict = numerical();
    conflict["inputs"]["input"]["bad_minutes"] = json!(21);
    assert_eq!(server.post("/api/v1/runs", &conflict).status, 409);
    assert_eq!(
        server
            .post(&format!("/api/v1/runs/{id}/cancel"), &json!({}))
            .status,
        200
    );
    let cleared = server.request(
        "DELETE",
        "/api/v1/runs",
        b"{}",
        true,
        true,
        "Content-Type: application/json\r\n",
    );
    assert_eq!(cleared.json(), json!({"removed":1,"active_preserved":true}));
    assert_eq!(
        server.get(&format!("/api/v1/runs/{id}/receipt")).status,
        410
    );
    assert_eq!(server.post("/api/v1/runs", &numerical()).json(), terminal);
    assert_eq!(server.get("/api/v1/runs").json()["runs"], json!([]));
}

#[test]
fn http_boundary_refuses_foreign_authority_credentials_and_mutation_origins() {
    let server = Server::start(&["task.run"]);
    assert_eq!(
        server
            .request("GET", "/api/v1/session", &[], false, false, "")
            .status,
        401
    );
    let foreign=server.raw(format!("GET /api/v1/session HTTP/1.1\r\nHost: attacker.invalid\r\nAuthorization: Bearer {}\r\n\r\n",server.token).as_bytes(),&[]);
    assert_eq!(foreign.status, 403);
    assert_eq!(
        server
            .request(
                "GET",
                "/api/v1/session",
                &[],
                true,
                false,
                "Origin: http://attacker.invalid\r\n"
            )
            .status,
        403
    );
    assert_eq!(
        server
            .request(
                "POST",
                "/api/v1/runs",
                b"{}",
                true,
                false,
                "Content-Type: application/json\r\n"
            )
            .status,
        403
    );
    assert_eq!(
        server
            .request(
                "POST",
                "/api/v1/runs",
                b"{}",
                true,
                true,
                "Content-Type: text/plain\r\n"
            )
            .status,
        415
    );
    let preflight = server.request(
        "OPTIONS",
        "/api/v1/runs",
        &[],
        false,
        false,
        "Origin: http://attacker.invalid\r\n",
    );
    assert_eq!(preflight.status, 403);
    assert!(
        !preflight
            .headers
            .to_ascii_lowercase()
            .contains("access-control-allow")
    );
    let wrong = server.request(
        "GET",
        "/api/v1/session",
        &[],
        false,
        false,
        "Authorization: Bearer invalid\r\n",
    );
    assert_eq!(wrong.status, 401);
    let duplicate = server.request(
        "GET",
        "/api/v1/session",
        &[],
        true,
        false,
        &format!("Host: {}\r\n", server.authority),
    );
    assert_eq!(duplicate.status, 400);
    let index = server.get("/history?run=inv-abc");
    assert_eq!(index.status, 200);
    let headers = index.headers.to_ascii_lowercase();
    assert!(headers.contains("frame-ancestors 'none'"));
    assert!(headers.contains("cache-control: no-store"));
    assert!(headers.contains("x-content-type-options: nosniff"));
    assert!(!String::from_utf8_lossy(&index.body).contains(&server.token));
}

#[test]
fn bounded_json_typed_inputs_and_hidden_grants_fail_before_admission() {
    let server = Server::start(&["task.run"]);
    for payload in [
        b"{\"operation\":\"task.run\",\"operation\":\"process.exec\"}".to_vec(),
        format!("{}0{}", "[".repeat(25), "]".repeat(25)).into_bytes(),
    ] {
        assert_eq!(
            server
                .request(
                    "POST",
                    "/api/v1/runs",
                    &payload,
                    true,
                    true,
                    "Content-Type: application/json\r\n"
                )
                .status,
            400
        );
    }
    let oversized=server.raw(format!("POST /api/v1/runs HTTP/1.1\r\nHost: {}\r\nOrigin: http://{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: 65537\r\n\r\n",server.authority,server.authority,server.token).as_bytes(),&[]);
    assert_eq!(oversized.status, 413);
    let mut injected = numerical();
    injected["inputs"]["input"]["file"] = json!("/etc/passwd");
    let invalid = server.post("/api/v1/runs", &injected);
    assert_eq!(invalid.status, 422);
    assert_eq!(invalid.json()["errors"][0]["field"], "inputs.input");
    for (field, value) in [
        ("inputs", json!(["error-budget",1,{"slo":99.9}])),
        ("limits", json!([30000, 1048576])),
    ] {
        let mut sequence = numerical();
        sequence[field] = value;
        assert_eq!(server.post("/api/v1/runs", &sequence).status, 422);
    }
    let mut sequence = numerical();
    sequence["inputs"]["input"] = json!([99.9]);
    assert_eq!(server.post("/api/v1/runs", &sequence).status, 422);
    let mut root = numerical();
    root["root_id"] = Value::Null;
    assert_eq!(server.post("/api/v1/runs", &root).status, 422);
    let mut denied = numerical();
    denied["operation"] = json!("process.exec");
    assert_eq!(server.post("/api/v1/runs", &denied).status, 403);
    let mut denied = numerical();
    denied["inputs"]["id"] = json!("dashboard-hygiene");
    assert_eq!(server.post("/api/v1/runs", &denied).status, 403);
    let mut extra = numerical();
    extra["config"] = json!("/etc/passwd");
    assert_eq!(server.post("/api/v1/runs", &extra).status, 422);
    for query in [
        "limit=0",
        "limit=51",
        "offset=50",
        "limit=1&limit=2",
        "secret=1",
    ] {
        assert_eq!(server.get(&format!("/api/v1/runs?{query}")).status, 400);
    }
    assert_eq!(server.get("/api/v1/runs").json()["runs"], json!([]));
    assert_eq!(server.get("/api/v1/runs/missing").status, 404);
    assert_eq!(server.get("/etc/passwd").status, 404);
    assert_eq!(
        server
            .request("GET", "/api/v1/session", b"{}", true, false, "")
            .status,
        400
    );
}

#[test]
fn empty_grants_expose_no_checks() {
    let server = Server::start(&[]);
    assert_eq!(
        server.get("/api/v1/session").json()["operations"],
        json!([])
    );
    assert_eq!(server.post("/api/v1/runs", &numerical()).status, 403);
}

#[test]
#[ignore = "requires the native Cally trusted-runtime and namespace lane"]
fn native_command_receipts_match_cli_and_policy_replacement_cannot_change_grants() {
    use sha2::{Digest, Sha256};
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
    assert_eq!(
        std::env::var("WORKBENCH_PROFILE_REQUIRED").as_deref(),
        Ok("1")
    );
    let base = std::env::temp_dir().join(workbench_core::result::new_id("gui-native"));
    let root = base.join("checkout");
    fs::create_dir_all(&root).unwrap();
    let policy_path = base.join("policy.json");
    let mut bindings = serde_json::Map::new();
    for name in ["git", "rg", "bwrap"] {
        let path = fs::canonicalize(
            std::env::var_os(format!("WORKBENCH_PROFILE_{}", name.to_ascii_uppercase()))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(format!("/usr/bin/{name}"))),
        )
        .unwrap();
        let digest = Sha256::digest(fs::read(&path).unwrap());
        let encoded = digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        bindings.insert(
            name.into(),
            json!({"path":path,"sha256":format!("sha256:{encoded}")}),
        );
    }
    let git = bindings["git"]["path"].as_str().unwrap();
    let initialized = Command::new(git)
        .args([
            "init",
            "--quiet",
            "--template=",
            "--object-format=sha1",
            "--initial-branch=fixture",
        ])
        .current_dir(&root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(initialized.status.success());
    fs::write(root.join("fixture.txt"), "synthetic content\n").unwrap();
    let mut policy = json!({"version":1,"profile":"linux-read-v1","roots":[{"id":"checkout","path":root}],"executables":bindings});
    fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    fs::set_permissions(&policy_path, fs::Permissions::from_mode(0o600)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_save"));
    command.arg("--read-policy").arg(&policy_path).args([
        "ui",
        "serve",
        "--allow",
        "process.exec",
        "--allow",
        "command.inspect",
        "--root",
        "checkout",
    ]);
    let server = Server::launch(command);
    assert_eq!(
        server.get("/api/v1/session").json()["roots"],
        json!([{"id":"checkout","label":"checkout"}])
    );
    let submission = |operation: &str, id: u8| json!({"submission_id":format!("00000000-0000-4000-8000-{id:012}"),"operation":operation,"operation_version":1,"root_id":"checkout","inputs":{"program":"git","args":["status","--short"]},"limits":{"timeout_ms":30000,"max_output_bytes":1048576}});
    for (index, operation) in ["command.inspect", "process.exec"].into_iter().enumerate() {
        let admission = server.post("/api/v1/runs", &submission(operation, index as u8));
        assert_eq!(admission.status, 202);
        let terminal = server.terminal(admission.json()["id"].as_str().unwrap());
        let receipt = server.get(terminal["receipt_url"].as_str().unwrap()).json();
        assert_eq!(receipt["execution"]["status"], "succeeded", "{receipt}");
        let mut cli = Command::new(env!("CARGO_BIN_EXE_save"));
        cli.arg("--json").arg("--read-policy").arg(&policy_path);
        if operation == "command.inspect" {
            cli.args(["command", "inspect"]);
        } else {
            cli.arg("exec");
        }
        let output = cli
            .arg("--cwd")
            .arg(&root)
            .args(["--", "git", "status", "--short"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let cli: Value = serde_json::from_slice(&output.stdout).unwrap();
        for field in [
            "operation",
            "operation_version",
            "target",
            "resolved_target",
            "execution",
            "effect_outcome",
            "assessment",
            "coverage",
            "output",
            "effective_limits",
            "record_mode",
        ] {
            assert_eq!(receipt[field], cli[field], "{field}");
        }
        assert_eq!(receipt["data"]["policy"], cli["data"]["policy"]);
        assert_eq!(receipt["data"]["tool"], cli["data"]["tool"]);
    }
    policy["roots"][0]["id"] = json!("replacement");
    fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    let admission = server.post("/api/v1/runs", &submission("process.exec", 2));
    assert_eq!(admission.status, 202);
    let terminal = server.terminal(admission.json()["id"].as_str().unwrap());
    let receipt = server.get(terminal["receipt_url"].as_str().unwrap()).json();
    assert_eq!(receipt["execution"]["status"], "denied");
    assert_eq!(receipt["errors"][0]["code"], "read_policy_changed");
    assert_eq!(receipt["effect_outcome"], "not_attempted");
    let mut injection = submission("process.exec", 3);
    injection["root_id"] = json!("replacement");
    assert_eq!(server.post("/api/v1/runs", &injection).status, 403);
    drop(server);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn transport_deadlines_and_connection_overload_are_bounded() {
    let server = Server::start(&["task.run"]);
    let started = Instant::now();
    let stalled=server.raw(format!("POST /api/v1/runs HTTP/1.1\r\nHost: {}\r\nOrigin: http://{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n",server.authority,server.authority,server.token).as_bytes(),&[]);
    assert_eq!(stalled.status, 408);
    assert_eq!(
        stalled.json()["type"],
        "urn:agenticsre:problem:body-timeout"
    );
    assert!(started.elapsed() < Duration::from_secs(4));
    let mut stalled_headers = TcpStream::connect(&server.authority).unwrap();
    stalled_headers
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    stalled_headers.write_all(b"GET / HTTP/1.1\r\n").unwrap();
    let started = Instant::now();
    let mut closed = Vec::new();
    stalled_headers.read_to_end(&mut closed).unwrap();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(closed.is_empty() || String::from_utf8_lossy(&closed).starts_with("HTTP/1.1 408"));
    let mut held = Vec::new();
    for _ in 0..16 {
        let mut socket = TcpStream::connect(&server.authority).unwrap();
        socket.write_all(b"GET / HTTP/1.1\r\n").unwrap();
        held.push(socket);
    }
    let overloaded = server.get("/api/v1/session");
    assert_eq!(overloaded.status, 429);
    assert!(
        overloaded
            .headers
            .to_ascii_lowercase()
            .contains("retry-after: 1")
    );
    assert_eq!(
        overloaded.json()["type"],
        "urn:agenticsre:problem:connection-limit"
    );
    drop(held);
}

#[test]
fn server_signals_preserve_cli_exit_status_contract() {
    for (signal, code) in [(libc::SIGINT, 130), (libc::SIGTERM, 143)] {
        let mut server = Server::start(&[]);
        // SAFETY: signal targets only this test's owned live child.
        assert_eq!(unsafe { libc::kill(server.child.id() as i32, signal) }, 0);
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if let Some(status) = server.child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(code));
                break;
            }
            assert!(Instant::now() < deadline, "signal shutdown deadline");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
