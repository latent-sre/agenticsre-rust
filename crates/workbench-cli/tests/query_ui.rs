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
    fn launch(fixture: &Fixture, grants: &[&str], query: bool) -> Self {
        let installed = fixture
            .directory
            .join(format!("installed-save-{}", next_id()));
        let staging = installed.with_extension("staging");
        fs::copy(https::SAVE, &staging).unwrap();
        fs::rename(staging, &installed).unwrap();
        let mut command = Command::new(installed);
        command.env_clear().current_dir(&fixture.directory);
        if query {
            command
                .env("SAVE_GRAFANA_FIXTURE", fixture.provider.to_string())
                .arg("--config")
                .arg(&fixture.config);
        }
        command.args(["ui", "serve"]);
        for grant in grants {
            command.args(["--allow", grant]);
        }
        if query {
            command.args(["--query-target", "fixture"]);
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

fn run() -> Value {
    let mut value = submission(
        "query.run",
        json!({"id":"prometheus.up-by-job","version":1,
        "datasource":"metrics-1","parameters":{"job":"fixture"},
        "window":{"kind":"absolute","from":"2026-10-02T00:00:00Z","to":"2026-10-02T00:01:00Z"}}),
    );
    value["target_id"] = json!("fixture");
    value
}

#[test]
fn saved_query_browser_catalog_is_offline_and_independently_granted() {
    for operation in ["queries.list", "queries.describe"] {
        let fixture = Fixture::new(vec![]);
        let browser = Browser::launch(&fixture, &[operation], false);
        let (status, session) = browser.request("GET", "/api/v1/session", None);
        assert_eq!(status, 200);
        assert_eq!(session["operations"].as_array().unwrap().len(), 1);
        assert_eq!(session["operations"][0]["id"], operation);
        assert_eq!(session["query_targets"], json!([]));
        let inputs = if operation == "queries.list" {
            json!({})
        } else {
            json!({"id":"prometheus.up-by-job","version":1})
        };
        let (_, receipt) = browser.submit(&submission(operation, inputs));
        assert_eq!(receipt["operation"], operation);
        assert_eq!(receipt["execution"]["status"], "succeeded");
        assert_eq!(browser.request("POST", "/api/v1/runs", Some(&run())).0, 403);
        assert!(fixture.requests().is_empty());
    }
}

#[test]
fn saved_query_browser_runs_installed_queries_preserves_history_and_refuses_drift() {
    let fixture = Fixture::new(vec![
        Reply::org(),
        Reply::datasource("prometheus"),
        Reply::query(),
        Reply::org(),
        Reply::datasource("loki"),
        Reply::query(),
    ]);
    let browser = Browser::launch(
        &fixture,
        &["queries.list", "queries.describe", "query.run"],
        true,
    );
    let (_, session) = browser.request("GET", "/api/v1/session", None);
    assert_eq!(session["targets"], json!([]));
    assert_eq!(
        session["query_targets"],
        json!([{"id":"fixture","label":"fixture"}])
    );
    let (_, definition) = browser.submit(&submission(
        "queries.describe",
        json!({"id":"prometheus.up-by-job","version":1}),
    ));
    let digest = &definition["data"]["query"]["definition_sha256"];
    assert!(fixture.requests().is_empty());
    let mut pinned = run();
    pinned["inputs"]["expected_definition_sha256"] = digest.clone();
    for (field, value) in [
        ("expr", json!("up")),
        ("expected_definition_sha256", Value::Null),
        ("parameters", json!({"job":"fixture","expr":"up"})),
        ("config", json!("/tmp/config.json")),
    ] {
        let mut bad = pinned.clone();
        bad["inputs"][field] = value;
        assert_eq!(browser.request("POST", "/api/v1/runs", Some(&bad)).0, 422);
    }
    let mut bad = pinned.clone();
    bad["target_id"] = json!("other");
    assert_eq!(browser.request("POST", "/api/v1/runs", Some(&bad)).0, 403);
    let mut mismatch = run();
    mismatch["inputs"]["expected_definition_sha256"] = json!("0".repeat(64));
    let (_, denied) = browser.submit(&mismatch);
    assert_eq!(denied["execution"]["status"], "denied");
    assert!(fixture.requests().is_empty());
    let (summary, receipt) = browser.submit(&pinned);
    assert_eq!(receipt["operation"], "query.run");
    assert_eq!(receipt["execution"]["status"], "succeeded", "{receipt}");
    assert_eq!(receipt["data"]["definition"]["sha256"], *digest);
    assert_eq!(receipt["data"]["request"]["expr"], "up{job=\"fixture\"}");
    assert_eq!(summary["target_id"], "fixture");
    assert!(summary["root_id"].is_null());
    assert_eq!(fixture.requests().len(), 3);
    let (status, retry) = browser.request("POST", "/api/v1/runs", Some(&pinned));
    assert_eq!(status, 202);
    assert_eq!(retry["id"], summary["id"]);
    let mut conflict = pinned.clone();
    conflict["inputs"]["parameters"]["job"] = json!("changed");
    assert_eq!(
        browser.request("POST", "/api/v1/runs", Some(&conflict)).0,
        409
    );
    let mut loki = run();
    loki["inputs"]["id"] = json!("loki.logs-by-job");
    loki["inputs"]["window"] = json!({"kind":"relative","lookback_seconds":60});
    let (_, receipt) = browser.submit(&loki);
    assert_eq!(receipt["execution"]["status"], "succeeded", "{receipt}");
    assert_eq!(receipt["data"]["request"]["expr"], "{job=\"fixture\"}");
    assert_eq!(fixture.requests().len(), 6);
    fixture.edit_config(|config| config["connections"]["fixture"]["expected_org_id"] = json!(8));
    let (_, denied) = browser.submit(&run());
    assert_eq!(denied["execution"]["status"], "denied");
    assert!(
        denied["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["code"] == "grafana_configuration_changed")
    );
    assert_eq!(fixture.requests().len(), 6);
    let (status, _) = browser.request("DELETE", "/api/v1/runs", Some(&json!({})));
    assert_eq!(status, 200);
    assert_eq!(
        browser
            .request("GET", summary["receipt_url"].as_str().unwrap(), None)
            .0,
        410
    );
    let (_, tombstone) = browser.request("POST", "/api/v1/runs", Some(&pinned));
    assert_eq!(tombstone["id"], summary["id"]);
    assert_eq!(fixture.requests().len(), 6);
}

#[test]
fn saved_query_browser_rejects_startup_authority_mismatches() {
    let fixture = Fixture::new(vec![]);
    for (grants, targets, config, exit) in [
        (vec!["query.run"], vec![], false, 1),
        (vec!["query.run"], vec!["fixture"], false, 1),
        (vec!["queries.list"], vec!["fixture"], true, 2),
        (vec!["query.run"], vec!["missing"], true, 1),
        (vec!["query.run"], vec!["fixture", "fixture"], true, 1),
    ] {
        let mut command = Command::new(https::SAVE);
        command.env_clear();
        if config {
            command.arg("--config").arg(&fixture.config);
        }
        command.args(["ui", "serve"]);
        for grant in grants {
            command.args(["--allow", grant]);
        }
        for target in targets {
            command.args(["--query-target", target]);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(exit));
        assert!(output.stdout.is_empty());
    }
    assert!(fixture.requests().is_empty());
}
