#![cfg(all(feature = "test-fixtures", target_os = "linux"))]
#[allow(dead_code)]
#[path = "support/grafana_https.rs"]
mod https;

use https::{Fixture, Reply};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::{fd::AsRawFd, unix::fs::PermissionsExt},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

fn inputs() -> Value {
    json!({"target_id":"fixture","service":"fixture-web","environment":"dev",
        "pack":{"id":"service-queries","version":1},
        "window":{"kind":"relative","lookback_seconds":300},
        "metrics":{"datasource":"metrics-1","parameters":{"job":"fixture"}},
        "logs":{"datasource":"logs-1","parameters":{"job":"fixture"}},"on_failure":"continue"})
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

struct Setup {
    fixture: Fixture,
    store: PathBuf,
    source: PathBuf,
    installed: PathBuf,
}
impl Setup {
    fn new() -> Self {
        let fixture = Fixture::new(replies());
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
        let source = fixture.directory.join("context.json");
        let mut context: Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/context/unique.json"))
                .unwrap();
        context["records"][0]["service"] = json!("fixture-web");
        context["records"][0]["last_reviewed"] = json!(workbench_core::result::timestamp());
        context["records"][0]["bindings"] =
            json!({"target":"fixture","environment":"dev","organization":"7"});
        fs::write(&source, context.to_string()).unwrap();
        let installed = fixture.directory.join("installed-save");
        https::install_save(&installed);
        Self {
            fixture,
            store,
            source,
            installed,
        }
    }
    fn launch(&self, grants: &[&str]) -> Client {
        let mut command = Command::new(&self.installed);
        command.env_clear().current_dir(&self.fixture.directory);
        if grants.contains(&"evidence.collect") {
            command
                .env("SAVE_GRAFANA_FIXTURE", self.fixture.provider.to_string())
                .arg("--config")
                .arg(&self.fixture.config)
                .arg("--context-source")
                .arg(&self.source)
                .args(["--context-max-age-days", "30"]);
        }
        if grants
            .iter()
            .any(|grant| matches!(*grant, "evidence.collect" | "evidence.open"))
        {
            command.arg("--state-dir").arg(&self.store);
        }
        command.args(["mcp", "serve"]);
        for grant in grants {
            command.args(["--allow", grant]);
        }
        if grants.contains(&"evidence.collect") {
            command.args(["--target", "fixture"]);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Client {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
        }
    }
}
struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}
impl Client {
    fn call(&mut self, method: &str, mut params: Value) -> Value {
        params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28", "io.modelcontextprotocol/clientCapabilities":{}});
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
        let mut fd = libc::pollfd {
            fd: self.output.get_ref().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if self.output.buffer().is_empty() {
            // SAFETY: poll borrows one initialized descriptor for this owned child pipe.
            assert!(unsafe { libc::poll(&mut fd, 1, 8000) } > 0);
        }
        let mut line = String::new();
        assert!(self.output.read_line(&mut line).unwrap() > 0);
        serde_json::from_str(&line).unwrap()
    }
    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        self.call("tools/call", json!({"name":name,"arguments":arguments}))
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn installed_collection_retains_bundle_and_separate_read_grant_opens_it() {
    let setup = Setup::new();
    let mut client = setup.launch(&[
        "evidence.collect",
        "evidence.packs.list",
        "evidence.packs.describe",
    ]);
    let discovery = client.call("tools/list", json!({}));
    let tools = discovery["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);
    let collect = tools
        .iter()
        .find(|tool| tool["name"] == "workbench_evidence_collect")
        .unwrap();
    assert_eq!(collect["annotations"]["readOnlyHint"], false);
    assert_eq!(collect["annotations"]["destructiveHint"], false);
    assert_eq!(collect["annotations"]["openWorldHint"], true);
    let catalog = client.tool("workbench_evidence_packs_list", json!({}));
    assert_eq!(
        catalog["result"]["structuredContent"]["data"]["packs"][0]["id"],
        "service-queries"
    );
    for field in ["state_dir", "record", "policy", "expr"] {
        let mut invalid = inputs();
        invalid[field] = json!("override");
        assert_eq!(
            client.tool("workbench_evidence_collect", invalid)["error"]["code"],
            -32602
        );
    }
    for (field, value) in [
        ("pack", json!({"id":"service-queries","version":2})),
        ("expected_pack_sha256", Value::Null),
        ("limits", json!({"timeout_ms":60001})),
        ("target_id", json!("other")),
    ] {
        let mut invalid = inputs();
        invalid[field] = value;
        assert_eq!(
            client.tool("workbench_evidence_collect", invalid)["error"]["code"],
            -32602
        );
    }
    assert!(setup.fixture.requests().is_empty());
    let collected = client.tool("workbench_evidence_collect", inputs());
    let result = &collected["result"]["structuredContent"];
    assert_eq!(result["execution"]["status"], "succeeded", "{collected}");
    assert_eq!(result["record_mode"], "always");
    assert_eq!(result["data"]["capture"]["state"], "complete");
    assert_eq!(setup.fixture.requests().len(), 6);
    let reference = json!({"id":result["data"]["bundle"]["id"], "expected_manifest_sha256":result["data"]["bundle"]["digest"]});
    assert_eq!(
        client.tool("workbench_evidence_open", reference.clone())["error"]["code"],
        -32602
    );
    drop(client);
    let mut reader = setup.launch(&["evidence.open"]);
    let opened = reader.tool("workbench_evidence_open", reference);
    assert_eq!(
        opened["result"]["structuredContent"]["execution"]["status"], "partial",
        "{opened}"
    );
    // Empty fixture frames retain a valid sealed bundle but cannot establish observations.
    assert_eq!(
        opened["result"]["structuredContent"]["data"]["observation"]["manifest_state"],
        "sealed"
    );
    assert_eq!(
        reader.tool("workbench_evidence_collect", inputs())["error"]["code"],
        -32602
    );
    assert_eq!(setup.fixture.requests().len(), 6);
}

#[test]
fn pinned_config_ca_source_and_store_drift_refuse_before_http_or_retention() {
    for drift in ["config", "ca", "source", "store", "missing-store"] {
        let setup = Setup::new();
        let mut client = setup.launch(&["evidence.collect"]);
        assert!(client.call("tools/list", json!({}))["result"]["tools"].is_array());
        match drift {
            "config" | "ca" | "source" => {
                let path = match drift {
                    "config" => &setup.fixture.config,
                    "ca" => &setup.fixture.ca,
                    _ => &setup.source,
                };
                let mut bytes = fs::read(path).unwrap();
                bytes.push(b' ');
                fs::write(path, bytes).unwrap();
            }
            _ => {
                fs::rename(&setup.store, setup.store.with_extension("old")).unwrap();
                if drift == "store" {
                    fs::create_dir(&setup.store).unwrap();
                    fs::set_permissions(&setup.store, fs::Permissions::from_mode(0o700)).unwrap();
                }
            }
        }
        let denied = client.tool("workbench_evidence_collect", inputs());
        assert_eq!(denied["result"]["isError"], true, "{drift}: {denied}");
        assert!(setup.fixture.requests().is_empty(), "{drift}");
        if drift == "missing-store" {
            assert!(!setup.store.exists());
        } else if drift == "store" {
            assert_eq!(fs::read_dir(&setup.store).unwrap().count(), 0);
        } else {
            assert_eq!(fs::read_dir(setup.store.join("runs")).unwrap().count(), 0);
        }
    }
}

#[test]
fn catalogs_are_independent_and_empty_authority_cannot_collect() {
    let setup = Setup::new();
    for grant in ["evidence.packs.list", "evidence.packs.describe"] {
        let mut client = setup.launch(&[grant]);
        assert_eq!(
            client.call("tools/list", json!({}))["result"]["tools"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let (name, input) = if grant.ends_with("list") {
            ("workbench_evidence_packs_list", json!({}))
        } else {
            (
                "workbench_evidence_packs_describe",
                json!({"id":"service-queries","version":1}),
            )
        };
        assert_eq!(client.tool(name, input)["result"]["isError"], false);
        assert_eq!(
            client.tool("workbench_evidence_collect", inputs())["error"]["code"],
            -32602
        );
    }
    let mut client = setup.launch(&[]);
    assert_eq!(
        client.call("tools/list", json!({}))["result"]["tools"],
        json!([])
    );
    assert_eq!(
        client.tool("workbench_evidence_collect", inputs())["error"]["code"],
        -32602
    );
    assert!(setup.fixture.requests().is_empty());
}
