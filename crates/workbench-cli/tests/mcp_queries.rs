#![cfg(all(feature = "test-fixtures", target_os = "linux"))]
#[allow(dead_code)]
#[path = "support/grafana_https.rs"]
mod https;

use https::{Fixture, Reply};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::fd::AsRawFd,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Client {
    fn launch(fixture: &Fixture, grants: &[&str], target: bool) -> Self {
        let installed = fixture.directory.join(format!(
            "installed-save-{}",
            workbench_core::result::new_id("query-mcp")
        ));
        https::install_save(&installed);
        let mut command = Command::new(installed);
        command.env_clear();
        command.current_dir(&fixture.directory);
        if target {
            command
                .env("SAVE_GRAFANA_FIXTURE", fixture.provider.to_string())
                .arg("--config")
                .arg(&fixture.config);
        }
        command.args(["mcp", "serve"]);
        for grant in grants {
            command.args(["--allow", grant]);
        }
        if target {
            command.args(["--target", "fixture"]);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
        }
    }

    fn call(&mut self, id: u64, method: &str, mut params: Value) -> Value {
        params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28",
            "io.modelcontextprotocol/clientCapabilities":{}});
        self.exchange(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
    }

    fn legacy(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.exchange(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
    }

    fn notify(&mut self, value: Value) {
        let input = self.input.as_mut().unwrap();
        input.write_all(value.to_string().as_bytes()).unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
    }

    fn exchange(&mut self, wire: Value) -> Value {
        let id = wire["id"].clone();
        let input = self.input.as_mut().unwrap();
        input.write_all(wire.to_string().as_bytes()).unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
        let mut fd = libc::pollfd {
            fd: self.output.get_ref().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if self.output.buffer().is_empty() {
            // SAFETY: poll observes one owned child pipe and does not mutate it.
            assert!(unsafe { libc::poll(&mut fd, 1, 5000) } > 0);
        }
        let mut line = String::new();
        assert!(self.output.read_line(&mut line).unwrap() > 0);
        let result: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(result["id"], id);
        result
    }

    fn stop(&mut self) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(7);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(0));
                return;
            }
            assert!(Instant::now() < deadline, "MCP shutdown timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tool(name: &str, arguments: Value) -> Value {
    json!({"name":name,"arguments":arguments})
}

#[test]
fn saved_query_catalog_grants_need_no_connection_or_credential() {
    let fixture = Fixture::new(vec![]);
    let mut client = Client::launch(&fixture, &["queries.list", "queries.describe"], false);
    let discovered = client.call(1, "tools/list", json!({}));
    let tools = discovered["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["name"], "workbench_queries_list");
    assert_eq!(tools[1]["name"], "workbench_queries_describe");
    let list = client.call(2, "tools/call", tool("workbench_queries_list", json!({})));
    assert_eq!(
        list["result"]["structuredContent"]["execution"]["status"],
        "succeeded"
    );
    let denied = client.call(3, "tools/call", tool("workbench_query_run", json!({})));
    assert_eq!(denied["error"]["code"], -32602);
    assert!(fixture.requests().is_empty());
    client.stop();
}

#[test]
fn saved_query_grants_discover_offline_and_run_only_the_installed_expression() {
    let fixture = Fixture::new(vec![
        Reply::org(),
        Reply::datasource("prometheus"),
        Reply::query(),
        Reply::org(),
        Reply::datasource("loki"),
        Reply::query(),
    ]);
    let mut client = Client::launch(
        &fixture,
        &["queries.list", "queries.describe", "query.run"],
        true,
    );
    let discovered = client.call(1, "tools/list", json!({}));
    let tools = discovered["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);
    assert_eq!(tools[0]["name"], "workbench_queries_list");
    assert_eq!(tools[1]["name"], "workbench_queries_describe");
    assert_eq!(tools[2]["name"], "workbench_query_run");
    assert_eq!(
        tools[2]["inputSchema"]["properties"]["target_id"]["enum"],
        json!(["fixture"])
    );
    let list = client.call(2, "tools/call", tool("workbench_queries_list", json!({})));
    assert_eq!(
        list["result"]["structuredContent"]["data"]["queries"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let describe = client.call(
        3,
        "tools/call",
        tool(
            "workbench_queries_describe",
            json!({"id":"prometheus.up-by-job","version":1}),
        ),
    );
    let digest = describe["result"]["structuredContent"]["data"]["query"]["definition_sha256"]
        .as_str()
        .unwrap();
    assert!(
        fixture.requests().is_empty(),
        "catalog calls must not contact Grafana"
    );
    let inputs = json!({"target_id":"fixture","id":"prometheus.up-by-job","version":1,
        "datasource":"metrics-1","parameters":{"job":"fixture"},
        "window":{"kind":"absolute","from":"2026-10-02T00:00:00Z","to":"2026-10-02T00:01:00Z"},
        "expected_definition_sha256":digest});
    let denied = client.call(4, "tools/call", tool("workbench_grafana_query", json!({})));
    assert_eq!(denied["error"]["code"], -32602);
    let denied = client.call(
        5,
        "tools/call",
        tool("workbench_query_run", json!({"target_id":"other"})),
    );
    assert_eq!(denied["error"]["code"], -32602);
    let mut null_pin = inputs.clone();
    null_pin["expected_definition_sha256"] = Value::Null;
    let denied = client.call(6, "tools/call", tool("workbench_query_run", null_pin));
    assert_eq!(denied["error"]["code"], -32602);
    let mut mismatch = inputs.clone();
    mismatch["expected_definition_sha256"] = json!("0".repeat(64));
    let denied = client.call(7, "tools/call", tool("workbench_query_run", mismatch));
    assert_eq!(denied["result"]["isError"], true);
    assert_eq!(
        denied["result"]["structuredContent"]["execution"]["status"],
        "denied"
    );
    assert!(fixture.requests().is_empty());
    let result = client.call(8, "tools/call", tool("workbench_query_run", inputs.clone()));
    assert_eq!(result["result"]["isError"], false, "{result}");
    let receipt = &result["result"]["structuredContent"];
    assert_eq!(receipt["operation"], "query.run");
    assert_eq!(receipt["execution"]["status"], "succeeded");
    assert_eq!(receipt["data"]["definition"]["sha256"], digest);
    assert_eq!(receipt["data"]["request"]["expr"], "up{job=\"fixture\"}");
    assert_eq!(fixture.requests().len(), 3);
    let initialized = client.legacy(
        9,
        "initialize",
        json!({"protocolVersion":"2025-11-25",
        "capabilities":{},"clientInfo":{"name":"query-fixture","version":"1"}}),
    );
    assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
    client.notify(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    let mut loki = inputs.clone();
    loki["id"] = json!("loki.logs-by-job");
    loki["expected_definition_sha256"] =
        json!("d4661dbe0efc261ccb5eb340cbcbceb90568fa2e0973b6518cfa4fd796eb6019");
    let result = client.legacy(10, "tools/call", tool("workbench_query_run", loki));
    assert!(result["result"].get("resultType").is_none());
    assert_eq!(result["result"]["isError"], false, "{result}");
    assert_eq!(
        result["result"]["structuredContent"]["data"]["request"]["expr"],
        "{job=\"fixture\"}"
    );
    assert_eq!(fixture.requests().len(), 6);
    fixture.edit_config(|config| config["connections"]["fixture"]["expected_org_id"] = json!(8));
    let denied = client.call(11, "tools/call", tool("workbench_query_run", inputs));
    assert_eq!(denied["result"]["isError"], true);
    assert_eq!(
        denied["result"]["structuredContent"]["execution"]["status"],
        "denied"
    );
    assert!(
        denied["result"]["structuredContent"]["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == "grafana_configuration_changed")
    );
    assert_eq!(fixture.requests().len(), 6);
    client.stop();
}
