#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};
use workbench_core::{EvidenceStoreOptions, OperatorContext, RunControl, request::Request};

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}
impl Client {
    fn spawn(args: &[&str]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_save"));
        command.args(["mcp", "serve"]).args(args);
        Self::launch(command)
    }
    fn launch(mut command: Command) -> Self {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
        }
    }
    fn send(&mut self, value: Value) {
        self.raw(&serde_json::to_string(&value).unwrap());
    }
    fn raw(&mut self, line: &str) {
        let input = self.input.as_mut().unwrap();
        input.write_all(line.as_bytes()).unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
    }
    fn read(&mut self) -> Value {
        use std::os::fd::AsRawFd;
        let mut fd = libc::pollfd {
            fd: self.output.get_ref().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if self.output.buffer().is_empty() {
            // SAFETY: polling one valid child stdout descriptor does not mutate flags or ownership.
            let ready = unsafe { libc::poll(&mut fd, 1, 5000) };
            assert!(ready > 0, "MCP reply timed out");
        }
        let mut line = String::new();
        assert!(
            self.output.read_line(&mut line).unwrap() > 0,
            "MCP closed unexpectedly"
        );
        serde_json::from_str(&line).unwrap()
    }
    fn stop(&mut self) -> i32 {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(7);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code().unwrap_or(-1);
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
fn request(id: Value, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}});
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
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
            std::env::temp_dir().join(workbench_core::result::new_id("investigation-mcp"));
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
    fn launch(&self, operations: &[&str]) -> Client {
        let mut command = Command::new(env!("CARGO_BIN_EXE_save"));
        if operations.contains(&"context.resolve") {
            command
                .arg("--context-source")
                .arg(&self.source)
                .args(["--context-max-age-days", "30"]);
        }
        if operations.iter().any(|op| {
            matches!(
                *op,
                "evidence.list" | "evidence.open" | "evidence.compare" | "report.create"
            )
        }) {
            command.arg("--state-dir").arg(&self.store);
        }
        command.args(["mcp", "serve"]);
        for operation in operations {
            command.args(["--allow", operation]);
        }
        Client::launch(command)
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
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in fs::read_dir(at).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                walk(root, &e.path(), out)
            } else {
                out.insert(
                    e.path().strip_prefix(root).unwrap().into(),
                    fs::read(e.path()).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}
const BEFORE: &str = "run-9-18dd60cacef0724e-1";
const AFTER: &str = "run-16-18dd60cad194352b-1";
const SHIFTED: &str = "run-23-18dd60cad423c07a-1";
const TOOLS: [&str; 5] = [
    "workbench_context_resolve",
    "workbench_evidence_list",
    "workbench_evidence_open",
    "workbench_compare",
    "workbench_report_create",
];
fn inputs() -> [Value; 5] {
    [
        json!({"service":"checkout","environment":"dev"}),
        json!({}),
        json!({"id":BEFORE}),
        json!({"before":{"id":BEFORE},"after":{"id":AFTER},"normalization":"exact-window-v1"}),
        json!({"bundles":[{"id":BEFORE},{"id":AFTER}],"format":"markdown"}),
    ]
}
fn call(client: &mut Client, name: &str, arguments: Value, legacy: bool) -> Value {
    let params = json!({"name":name,"arguments":arguments});
    client.send(if legacy {
        json!({"jsonrpc":"2.0","id":"call","method":"tools/call","params":params})
    } else {
        request(json!("call"), "tools/call", params)
    });
    let result = client.read();
    assert_eq!(result["id"], "call");
    result
}
fn canonical(reply: Value, operation: &str) -> Value {
    assert!(reply.get("error").is_none(), "{reply}");
    let result = &reply["result"];
    let receipt = &result["structuredContent"];
    assert_eq!(receipt["operation"], operation, "{reply}");
    assert_eq!(receipt["record_mode"], "never");
    assert_eq!(receipt["assessment"], "not_assessed");
    assert_eq!(
        result["isError"],
        receipt["execution"]["status"] != "succeeded",
        "{reply}"
    );
    assert_eq!(result["content"].as_array().unwrap().len(), 1);
    assert_eq!(result["content"][0]["type"], "text");
    assert!(result["content"][0]["text"].as_str().unwrap().len() <= 512);
    receipt.clone()
}
fn discovery(client: &mut Client) -> Vec<String> {
    client.send(request(json!("list"), "tools/list", json!({})));
    let listed = client.read();
    listed["result"]["tools"]
        .as_array()
        .expect("tool discovery")
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn both_protocols_return_unchanged_core_investigations_and_honest_partial_receipts() {
    let fixture = Fixture::new();
    let original = snapshot(&fixture.store);
    let grant = workbench_core::investigation_grant(
        &fixture.context(),
        &OPERATIONS.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
    .unwrap();
    for legacy in [false, true] {
        let mut client = fixture.launch(&OPERATIONS);
        let mut names = discovery(&mut client);
        names.sort();
        let mut expected_names = TOOLS.map(str::to_owned);
        expected_names.sort();
        assert_eq!(names, expected_names);
        if legacy {
            client.send(json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"investigation-fixture","version":"1"}}}));
            assert_eq!(client.read()["result"]["protocolVersion"], "2025-11-25");
            client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        }
        for ((operation, tool), input) in OPERATIONS.into_iter().zip(TOOLS).zip(inputs()) {
            let reply = call(&mut client, tool, input.clone(), legacy);
            assert_eq!(reply["result"].get("resultType").is_none(), legacy);
            let actual = canonical(reply, operation);
            assert_eq!(actual["execution"]["status"], "succeeded", "{actual}");
            let mut request = Request::new(operation, input);
            if matches!(operation, "evidence.open" | "evidence.compare") {
                request.limits.max_items = Some(100);
            }
            let expected = serde_json::to_value(workbench_core::execute_with_investigation_grant(
                request,
                &RunControl::default(),
                &grant,
            ))
            .unwrap();
            assert_eq!(
                stable(actual),
                stable(expected),
                "{operation}, legacy={legacy}"
            );
        }
        for (tool, operation, input, status, detail) in [
            (
                "workbench_evidence_open",
                "evidence.open",
                json!({"id":BEFORE,"limits":{"max_items":1,"max_output_bytes":1024}}),
                "partial",
                None,
            ),
            (
                "workbench_report_create",
                "report.create",
                json!({"bundles":[{"id":BEFORE}],"format":"json","limits":{"max_output_bytes":1024}}),
                "partial",
                None,
            ),
            (
                "workbench_compare",
                "evidence.compare",
                json!({"before":{"id":BEFORE},"after":{"id":SHIFTED},"normalization":"exact-window-v1"}),
                "partial",
                None,
            ),
            (
                "workbench_evidence_open",
                "evidence.open",
                json!({"id":BEFORE,"expected_manifest_sha256":format!("sha256:{}","0".repeat(64))}),
                "failed",
                Some("manifest_digest_mismatch"),
            ),
        ] {
            let result = canonical(call(&mut client, tool, input, legacy), operation);
            assert_eq!(result["execution"]["status"], status, "{result}");
            if let Some(code) = detail {
                assert_eq!(result["errors"][0]["code"], code);
            }
        }
        assert_eq!(client.stop(), 0);
        assert_eq!(snapshot(&fixture.store), original);
    }
}

#[test]
fn grants_are_independent_and_mix_with_existing_numerical_work() {
    let fixture = Fixture::new();
    let original = snapshot(&fixture.store);
    for ((operation, tool), input) in OPERATIONS.into_iter().zip(TOOLS).zip(inputs()) {
        let mut client = fixture.launch(&[operation]);
        assert_eq!(discovery(&mut client), vec![tool]);
        let result = canonical(call(&mut client, tool, input, false), operation);
        assert_eq!(result["execution"]["status"], "succeeded", "{result}");
        for hidden in TOOLS.into_iter().filter(|name| *name != tool) {
            assert_eq!(
                call(&mut client, hidden, json!({}), false)["error"]["code"],
                -32602
            );
        }
        assert_eq!(client.stop(), 0);
    }
    let mut empty = Client::spawn(&[]);
    assert!(discovery(&mut empty).is_empty());
    for tool in TOOLS {
        assert_eq!(
            call(&mut empty, tool, json!({}), false)["error"]["code"],
            -32602
        );
    }
    assert_eq!(empty.stop(), 0);
    let mut mixed = fixture.launch(&["evidence.list", "task.run"]);
    let names = discovery(&mut mixed);
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"workbench_evidence_list".into()));
    assert!(names.contains(&"workbench_task_run".into()));
    for (tool, operation, input) in [
        ("workbench_evidence_list", "evidence.list", json!({})),
        (
            "workbench_task_run",
            "task.run",
            json!({"id":"error-budget","version":1,"input":{"slo":99.9,"window_days":28,"bad_minutes":20}}),
        ),
    ] {
        let result = canonical(call(&mut mixed, tool, input, false), operation);
        assert_eq!(result["execution"]["status"], "succeeded", "{result}");
    }
    assert_eq!(mixed.stop(), 0);
    assert_eq!(snapshot(&fixture.store), original);
}

#[test]
fn client_arguments_cannot_replace_authority_or_bypass_operation_limits() {
    let fixture = Fixture::new();
    let original = snapshot(&fixture.store);
    let mut client = fixture.launch(&OPERATIONS);
    assert_eq!(discovery(&mut client).len(), 5);
    for ((operation, tool), input) in OPERATIONS.into_iter().zip(TOOLS).zip(inputs()) {
        for (field, value) in [
            ("path", json!("/tmp/not-authorized")),
            ("context_source", json!("/tmp/not-authorized")),
            ("state_dir", json!("/tmp/not-authorized")),
            ("root_id", Value::Null),
            ("target_id", Value::Null),
            ("role", json!("operator")),
            ("record", json!("always")),
        ] {
            let mut bad = input.clone();
            bad[field] = value;
            assert_eq!(
                call(&mut client, tool, bad, false)["error"]["code"],
                -32602,
                "{operation}, {field}"
            );
        }
        for limits in [
            Value::Null,
            json!({"timeout_ms":0}),
            json!({"timeout_ms":300001}),
            json!({"timeout_ms":null}),
            json!({"max_output_bytes":1023}),
            json!({"max_output_bytes":1048577}),
            json!({"max_output_bytes":true}),
            json!({"concurrency":1}),
            json!({"max_items":0}),
            json!({"max_items":1001}),
            json!({"max_items":null}),
            json!({"unexpected":1}),
        ] {
            let mut bad = input.clone();
            bad["limits"] = limits;
            assert_eq!(
                call(&mut client, tool, bad, false)["error"]["code"],
                -32602,
                "{operation}"
            );
        }
        let mut bounded = input;
        bounded["limits"] = json!({"timeout_ms":300000,"max_output_bytes":1048576,"max_items":1});
        if matches!(operation, "evidence.open" | "evidence.compare") {
            let result = canonical(call(&mut client, tool, bounded, false), operation);
            assert_eq!(result["effective_limits"]["max_items"], 1);
            assert_eq!(result["effective_limits"]["timeout_ms"], 300000);
        } else {
            assert_eq!(
                call(&mut client, tool, bounded, false)["error"]["code"],
                -32602
            );
        }
    }
    for (tool, bad) in [
        (
            "workbench_context_resolve",
            json!({"service":"checkout","environment":"dev","team":null}),
        ),
        ("workbench_evidence_open", json!({"id":"../escape"})),
        (
            "workbench_evidence_open",
            json!({"id":BEFORE,"expected_manifest_sha256":null}),
        ),
        (
            "workbench_compare",
            json!({"before":{"id":BEFORE,"path":"/tmp/x"},"after":{"id":AFTER},"normalization":"exact-window-v1"}),
        ),
        (
            "workbench_report_create",
            json!({"bundles":[{"id":BEFORE}],"format":"json","title":null}),
        ),
    ] {
        assert_eq!(call(&mut client, tool, bad, false)["error"]["code"], -32602);
    }
    for hidden in [
        "workbench_evidence_collect",
        "workbench_evidence_replay",
        "workbench_evidence_delete",
        "workbench_change_apply",
    ] {
        assert_eq!(
            call(&mut client, hidden, json!({}), false)["error"]["code"],
            -32602
        );
    }
    assert_eq!(client.stop(), 0);
    assert_eq!(snapshot(&fixture.store), original);
}

#[test]
fn source_bytes_source_inode_and_store_replacement_return_canonical_refusals() {
    let fixture = Fixture::new();
    let original = snapshot(&fixture.store);
    let source = fs::read(&fixture.source).unwrap();
    let mut client = fixture.launch(&OPERATIONS);
    assert_eq!(discovery(&mut client).len(), 5);
    fs::write(&fixture.source, [&source[..], b" "].concat()).unwrap();
    let denied = canonical(
        call(&mut client, TOOLS[0], inputs()[0].clone(), false),
        OPERATIONS[0],
    );
    assert_eq!(denied["errors"][0]["code"], "context_source_changed");
    assert!(denied["data"].get("record").is_none());
    fs::rename(&fixture.source, fixture.directory.join("old-source")).unwrap();
    fs::write(&fixture.source, &source).unwrap();
    fs::set_permissions(&fixture.source, fs::Permissions::from_mode(0o600)).unwrap();
    let denied = canonical(
        call(&mut client, TOOLS[0], inputs()[0].clone(), false),
        OPERATIONS[0],
    );
    assert_eq!(denied["errors"][0]["code"], "context_source_changed");
    assert!(denied["data"].get("record").is_none());
    let old = fixture.directory.join("old-store");
    fs::rename(&fixture.store, &old).unwrap();
    copy_store(&old, &fixture.store);
    for ((operation, tool), input) in OPERATIONS.into_iter().zip(TOOLS).zip(inputs()).skip(1) {
        let denied = canonical(call(&mut client, tool, input, false), operation);
        assert_eq!(
            denied["errors"][0]["code"], "store_identity_changed",
            "{denied}"
        );
        for field in ["runs", "manifest", "report"] {
            assert!(denied["data"].get(field).is_none());
        }
    }
    assert_eq!(client.stop(), 0);
    assert_eq!(snapshot(&fixture.store), original);
    assert_eq!(snapshot(&old), original);
}

#[test]
fn invalid_startup_keeps_stdout_empty_exits_two_and_never_creates_stores() {
    let fixture = Fixture::new();
    let original = snapshot(&fixture.store);
    let source = fixture.source.to_str().unwrap();
    let store = fixture.store.to_str().unwrap();
    let missing_path = fixture.directory.join("must-not-exist");
    let missing = missing_path.to_str().unwrap();
    let invalid_path = fixture.directory.join("invalid.json");
    fs::write(&invalid_path, b"{}").unwrap();
    let invalid = invalid_path.to_str().unwrap();
    let empty_path = fixture.directory.join("empty-store");
    fs::create_dir(&empty_path).unwrap();
    fs::set_permissions(&empty_path, fs::Permissions::from_mode(0o700)).unwrap();
    let empty = empty_path.to_str().unwrap();
    let mut cases = vec![
        vec!["--context-source", source, "--context-max-age-days", "30"],
        vec!["--state-dir", store],
        vec!["--context-source", source, "--allow", "context.resolve"],
        vec!["--context-max-age-days", "30", "--allow", "context.resolve"],
        vec![
            "--context-source",
            missing,
            "--context-max-age-days",
            "30",
            "--allow",
            "context.resolve",
        ],
        vec![
            "--context-source",
            invalid,
            "--context-max-age-days",
            "30",
            "--allow",
            "context.resolve",
        ],
        vec![
            "--context-source",
            source,
            "--context-max-age-days",
            "366",
            "--allow",
            "context.resolve",
        ],
        vec!["--state-dir", missing, "--allow", "evidence.list"],
        vec!["--state-dir", empty, "--allow", "evidence.list"],
        vec![
            "--state-dir",
            store,
            "--store-max-bytes",
            "1048576",
            "--allow",
            "evidence.list",
        ],
        vec![
            "--state-dir",
            store,
            "--retention-days",
            "1",
            "--allow",
            "evidence.list",
        ],
        vec![
            "--state-dir",
            store,
            "--record",
            "always",
            "--allow",
            "evidence.list",
        ],
        vec![
            "--state-dir",
            store,
            "--allow",
            "evidence.list",
            "--allow",
            "evidence.list",
        ],
        vec!["--state-dir", store, "--allow", "evidence.delete"],
        vec!["--state-dir", store, "--json", "--allow", "evidence.list"],
        vec![
            "--state-dir",
            store,
            "--runbook-state-dir",
            missing,
            "--allow",
            "evidence.list",
        ],
        vec![
            "--state-dir",
            store,
            "--diagnostics-config",
            missing,
            "--allow",
            "evidence.list",
        ],
    ];
    for operation in OPERATIONS {
        cases.push(vec!["--allow", operation]);
    }
    for args in cases {
        let output = Command::new(env!("CARGO_BIN_EXE_save"))
            .args(["mcp", "serve"])
            .args(&args)
            .stdin(Stdio::piped())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "startup wrote protocol noise");
        assert!(!output.stderr.is_empty());
        assert!(output.stderr.len() <= 512);
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains(fixture.directory.to_str().unwrap())
        );
    }
    assert!(!missing_path.exists());
    assert_eq!(fs::read_dir(empty_path).unwrap().count(), 0);
    assert_eq!(snapshot(&fixture.store), original);
}
