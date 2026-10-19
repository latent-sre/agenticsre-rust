#![cfg(all(feature = "test-fixtures", target_os = "linux"))]
#[allow(dead_code)]
#[path = "support/grafana_https.rs"]
mod https;

use https::{Fixture, Reply};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Browser {
    child: Child,
    authority: String,
    token: String,
}

impl Browser {
    fn launch(fixture: &Fixture, grants: &[&str], collect: bool) -> Self {
        let installed = fixture
            .directory
            .join(format!("installed-save-{}", next_id()));
        https::install_save(&installed);
        let mut command = Command::new(installed);
        command.env_clear().current_dir(&fixture.directory);
        if collect {
            command
                .env("SAVE_GRAFANA_FIXTURE", fixture.provider.to_string())
                .arg("--config")
                .arg(&fixture.config)
                .arg("--context-source")
                .arg(fixture.directory.join("context.json"))
                .args(["--context-max-age-days", "30"]);
        }
        if collect || grants.contains(&"evidence.open") {
            command
                .arg("--state-dir")
                .arg(fixture.directory.join("store"));
        }
        command.args(["ui", "serve"]);
        for grant in grants {
            command.args(["--allow", grant]);
        }
        if collect {
            command.args(["--collection-target", "fixture"]);
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut link = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut link)
            .unwrap();
        let (origin, token) = link.trim().split_once("/#token=").expect("launch link");
        Self {
            child,
            authority: origin.strip_prefix("http://").unwrap().to_owned(),
            token: token.into(),
        }
    }

    fn request(&self, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
        let body = body.map(Value::to_string).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nOrigin: http://{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.authority,
            self.token,
            self.authority,
            body.len()
        );
        let mut socket = TcpStream::connect(&self.authority).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(6)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket.write_all(request.as_bytes()).unwrap();
        let mut bytes = vec![];
        socket
            .take(3 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .unwrap();
        let split = bytes.windows(4).position(|s| s == b"\r\n\r\n").unwrap();
        let headers = std::str::from_utf8(&bytes[..split]).unwrap();
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, serde_json::from_slice(&bytes[split + 4..]).unwrap())
    }

    fn submit(&self, submission: &Value) -> (Value, Value) {
        let (status, admitted) = self.request("POST", "/api/v1/runs", Some(submission));
        assert_eq!(status, 202, "{admitted}");
        let path = format!("/api/v1/runs/{}", admitted["id"].as_str().unwrap());
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let (status, summary) = self.request("GET", &path, None);
            assert_eq!(status, 200);
            if summary["state"] == "terminal" {
                let (status, receipt) = self.request("GET", &format!("{path}/receipt"), None);
                assert_eq!(status, 200);
                return (summary, receipt);
            }
            assert!(Instant::now() < deadline, "receipt deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        // SAFETY: signal targets the owned child process, with no borrowed pointers.
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if self.child.try_wait().unwrap().is_some() {
                return;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!("UI shutdown deadline");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn submission(operation: &str, inputs: Value) -> Value {
    json!({"submission_id":format!("12345678-1234-4234-8234-{:012x}", next_id()),
        "operation":operation,"operation_version":1,"inputs":inputs,
        "limits":{"timeout_ms":30000,"max_output_bytes":1048576}})
}

fn next_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn initialize(fixture: &Fixture) {
    use std::os::unix::fs::PermissionsExt;
    let store = fixture.directory.join("store");
    fs::create_dir(&store).unwrap();
    fs::create_dir(store.join("runs")).unwrap();
    for path in [&store, &store.join("runs")] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    for (name, raw) in [("format.json", "{\"format_version\":1}\n"), ("lock", "")] {
        let path = store.join(name);
        fs::write(&path, raw).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut context: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/context/unique.json")).unwrap();
    context["records"][0]["service"] = json!("fixture-web");
    context["records"][0]["last_reviewed"] = json!(workbench_core::result::timestamp());
    context["records"][0]["bindings"] =
        json!({"target":"fixture","environment":"dev","organization":"7"});
    fs::write(fixture.directory.join("context.json"), context.to_string()).unwrap();
}

fn run() -> Value {
    let mut value = submission(
        "evidence.collect",
        json!({"service":"fixture-web","environment":"dev",
        "pack":{"id":"service-queries","version":1},"window":{"kind":"relative","lookback_seconds":300},
        "metrics":{"datasource":"metrics-1","parameters":{"job":"fixture"}},
        "logs":{"datasource":"logs-1","parameters":{"job":"fixture"}},"on_failure":"continue"}),
    );
    value["target_id"] = json!("fixture");
    value
}

fn replies() -> Vec<Reply> {
    vec![
        Reply::org(),
        Reply::json(json!({"uid":"metrics-1","type":"prometheus","orgId":7})),
        Reply::query(),
        Reply::org(),
        Reply::json(json!({"uid":"logs-1","type":"loki","orgId":7})),
        Reply::query(),
    ]
}

#[test]
fn pack_catalog_grants_are_independent_offline_and_cannot_collect() {
    for operation in ["evidence.packs.list", "evidence.packs.describe"] {
        let fixture = Fixture::new(vec![]);
        let browser = Browser::launch(&fixture, &[operation], false);
        let (status, session) = browser.request("GET", "/api/v1/session", None);
        assert_eq!(status, 200);
        assert_eq!(session["operations"].as_array().unwrap().len(), 1);
        assert_eq!(session["operations"][0]["id"], operation);
        assert_eq!(session["collection_targets"], json!([]));
        let inputs = if operation.ends_with("list") {
            json!({})
        } else {
            json!({"id":"service-queries","version":1})
        };
        let (_, receipt) = browser.submit(&submission(operation, inputs));
        assert_eq!(receipt["execution"]["status"], "succeeded");
        assert_eq!(receipt["record_mode"], "never");
        assert_eq!(browser.request("POST", "/api/v1/runs", Some(&run())).0, 403);
        assert!(fixture.requests().is_empty());
        assert!(!fixture.directory.join("store").exists());
    }
}

#[test]
fn collection_seals_evidence_preserves_retry_identity_and_needs_separate_read_grant() {
    let fixture = Fixture::new(replies());
    initialize(&fixture);
    let browser = Browser::launch(
        &fixture,
        &["evidence.collect", "evidence.packs.describe"],
        true,
    );
    let (_, session) = browser.request("GET", "/api/v1/session", None);
    assert_eq!(session["query_targets"], json!([]));
    assert_eq!(session["targets"], json!([]));
    assert_eq!(
        session["collection_targets"],
        json!([{"id":"fixture","label":"fixture"}])
    );
    let (_, metadata) = browser.submit(&submission(
        "evidence.packs.describe",
        json!({"id":"service-queries","version":1}),
    ));
    assert_eq!(metadata["execution"]["status"], "succeeded");
    let mut pinned = run();
    pinned["inputs"]["expected_pack_sha256"] = metadata["data"]["pack"]["sha256"].clone();
    assert!(pinned["inputs"]["expected_pack_sha256"].is_string());
    for field in ["state_dir", "record", "policy", "expr"] {
        let mut bad = pinned.clone();
        bad["inputs"][field] = json!("override");
        assert_eq!(browser.request("POST", "/api/v1/runs", Some(&bad)).0, 422);
    }
    for (field, value) in [
        ("root_id", Value::Null),
        ("target_id", Value::Null),
        ("record", json!("never")),
    ] {
        let mut bad = pinned.clone();
        bad[field] = value;
        assert_eq!(browser.request("POST", "/api/v1/runs", Some(&bad)).0, 422);
    }
    for (field, value) in [
        ("timeout_ms", json!(60001)),
        ("concurrency", json!(1)),
        ("max_items", json!(10)),
    ] {
        let mut bad = pinned.clone();
        bad["limits"][field] = value;
        assert_eq!(browser.request("POST", "/api/v1/runs", Some(&bad)).0, 422);
    }
    let mut bad = run();
    bad["target_id"] = json!("other");
    assert_eq!(browser.request("POST", "/api/v1/runs", Some(&bad)).0, 403);
    let mut bad = run();
    bad["inputs"]["expected_pack_sha256"] = Value::Null;
    assert_eq!(browser.request("POST", "/api/v1/runs", Some(&bad)).0, 422);
    let mut mismatch = run();
    mismatch["inputs"]["expected_pack_sha256"] = json!("0".repeat(64));
    assert_eq!(
        browser.request("POST", "/api/v1/runs", Some(&mismatch)).0,
        422
    );
    assert!(fixture.requests().is_empty());
    let (summary, receipt) = browser.submit(&pinned);
    assert_eq!(receipt["execution"]["status"], "succeeded", "{receipt}");
    assert_eq!(receipt["record_mode"], "always");
    assert_eq!(receipt["data"]["capture"]["state"], "complete");
    assert_eq!(fixture.requests().len(), 6);
    let reference = json!({"id":receipt["data"]["bundle"]["id"],"expected_manifest_sha256":receipt["data"]["bundle"]["digest"]});
    let (status, retry) = browser.request("POST", "/api/v1/runs", Some(&pinned));
    assert_eq!(status, 202);
    assert_eq!(retry["id"], summary["id"]);
    let mut conflict = pinned.clone();
    conflict["inputs"]["metrics"]["parameters"]["job"] = json!("changed");
    assert_eq!(
        browser.request("POST", "/api/v1/runs", Some(&conflict)).0,
        409
    );
    assert_eq!(
        browser
            .request(
                "POST",
                "/api/v1/runs",
                Some(&submission("evidence.open", reference.clone()))
            )
            .0,
        403
    );
    drop(browser);
    let reader = Browser::launch(&fixture, &["evidence.open"], false);
    let (_, opened) = reader.submit(&submission("evidence.open", reference));
    assert_eq!(opened["data"]["observation"]["manifest_state"], "sealed");
    assert_eq!(reader.request("POST", "/api/v1/runs", Some(&run())).0, 403);
    assert_eq!(fixture.requests().len(), 6);
}

#[test]
fn pinned_collection_resources_refuse_drift_before_network_or_store_creation() {
    for drift in ["config", "ca", "source", "store", "missing-store"] {
        let fixture = Fixture::new(replies());
        initialize(&fixture);
        let browser = Browser::launch(&fixture, &["evidence.collect"], true);
        let store = fixture.directory.join("store");
        if matches!(drift, "store" | "missing-store") {
            fs::rename(&store, store.with_extension("old")).unwrap();
            if drift == "store" {
                fs::create_dir(&store).unwrap();
            }
        } else {
            let source = fixture.directory.join("context.json");
            let path = match drift {
                "config" => &fixture.config,
                "ca" => &fixture.ca,
                _ => &source,
            };
            let mut bytes = fs::read(path).unwrap();
            bytes.push(b' ');
            fs::write(path, bytes).unwrap();
        }
        let (_, result) = browser.submit(&run());
        assert_ne!(
            result["execution"]["status"], "succeeded",
            "{drift}: {result}"
        );
        assert!(fixture.requests().is_empty(), "{drift}");
        if drift == "missing-store" {
            assert!(!store.exists());
        } else if drift == "store" {
            assert_eq!(fs::read_dir(&store).unwrap().count(), 0);
        } else {
            assert_eq!(fs::read_dir(store.join("runs")).unwrap().count(), 0);
        }
    }
}

#[test]
fn collection_startup_requires_all_prerequisites_and_its_own_target_scope() {
    for fault in [
        "config",
        "context",
        "age",
        "store",
        "missing-store",
        "target",
        "query-target",
        "duplicate-target",
        "no-collection-grant",
        "policy",
    ] {
        let fixture = Fixture::new(vec![]);
        initialize(&fixture);
        let installed = fixture.directory.join("installed-save");
        https::install_save(&installed);
        let mut args = Vec::new();
        for (name, flag, value) in [
            (
                "config",
                "--config",
                fixture.config.to_string_lossy().into_owned(),
            ),
            (
                "context",
                "--context-source",
                fixture
                    .directory
                    .join("context.json")
                    .to_string_lossy()
                    .into_owned(),
            ),
            ("age", "--context-max-age-days", "30".into()),
            (
                "store",
                "--state-dir",
                fixture
                    .directory
                    .join("missing-store")
                    .to_string_lossy()
                    .into_owned(),
            ),
        ] {
            if fault != name {
                args.extend([flag.to_owned(), value]);
            }
        }
        // All cases must refuse; the missing-store case also proves startup cannot initialize it.
        if fault != "missing-store"
            && let Some(index) = args.iter().position(|item| item == "--state-dir")
        {
            args[index + 1] = fixture
                .directory
                .join("store")
                .to_string_lossy()
                .into_owned();
        }
        if fault == "policy" {
            args.extend(["--store-max-bytes".into(), "1000000".into()]);
        }
        args.extend([
            "ui".into(),
            "serve".into(),
            "--allow".into(),
            if fault == "no-collection-grant" {
                "evidence.packs.list".into()
            } else {
                "evidence.collect".into()
            },
        ]);
        if fault == "query-target" {
            args.extend([
                "--allow".into(),
                "query.run".into(),
                "--query-target".into(),
                "fixture".into(),
            ]);
        } else if fault != "target" {
            args.extend(["--collection-target".into(), "fixture".into()]);
        }
        if fault == "duplicate-target" {
            args.extend(["--collection-target".into(), "fixture".into()]);
        }
        let mut child = Command::new(installed)
            .env_clear()
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() > deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("startup unexpectedly admitted {fault}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success(), "{fault}");
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("/#token="),
            "{fault} exposed a launch link"
        );
        assert!(fixture.requests().is_empty());
        assert!(!fixture.directory.join("missing-store").exists());
        assert_eq!(
            fs::read_dir(fixture.directory.join("store/runs"))
                .unwrap()
                .count(),
            0
        );
    }
}
