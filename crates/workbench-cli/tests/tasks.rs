#![cfg(all(feature = "test-fixtures", target_os = "linux"))]

use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const SAVE: &str = env!("CARGO_BIN_EXE_save");
const CLEAN: &str = include_str!("../../../tests/fixtures/dashboard-hygiene/clean.json");

struct Fixture {
    directory: PathBuf,
    model: PathBuf,
    input: PathBuf,
}
impl Fixture {
    fn new(content: &[u8]) -> Self {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "workbench-task-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let model = directory.join("model.json");
        let input = directory.join("input.json");
        fs::write(&model, content).unwrap();
        fs::write(&input, json!({"file":model}).to_string()).unwrap();
        Self {
            directory,
            model,
            input,
        }
    }
    fn set(&self, model: &Value) {
        fs::write(&self.model, model.to_string()).unwrap();
    }
    fn command(&self) -> Command {
        let mut command = Command::new(SAVE);
        command
            .args(["--json", "task", "run", "dashboard-hygiene", "--input"])
            .arg(&self.input);
        command
    }
    fn run(&self) -> (Output, Value) {
        let output = self.command().output().unwrap();
        let result = receipt(&output);
        (output, result)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn clean() -> Value {
    serde_json::from_str(CLEAN).unwrap()
}
fn receipt(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stdout)))
}
fn rules(result: &Value) -> BTreeSet<&str> {
    result["data"]["findings"]
        .as_array()
        .unwrap_or_else(|| panic!("missing findings: {result}"))
        .iter()
        .map(|f| f["rule"].as_str().unwrap())
        .collect()
}
fn error(result: &Value, code: &str) -> bool {
    result["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["code"] == code)
}

#[test]
fn clean_check_is_completed_observation_with_source_and_input_identity() {
    let fixture = Fixture::new(CLEAN.as_bytes());
    let (output, result) = fixture.run();
    assert_eq!(output.status.code(), Some(0), "{result}");
    assert!(rules(&result).is_empty());
    assert_eq!(result["execution"]["status"], "succeeded");
    assert_eq!(result["execution"]["child_exit_code"], 0);
    assert_eq!(result["effect_outcome"], "not_applicable");
    assert_eq!(result["resolved_target"]["effects"], "observe");
    assert_eq!(result["assessment"], "not_assessed");
    assert_eq!(result["data"]["checked_panels"], 1);
    assert_eq!(result["data"]["findings_total"], 0);
    assert_eq!(result["data"]["input"]["bytes"], CLEAN.len());
    assert_eq!(
        result["data"]["source"]["checker_digest"],
        "sha256:8a109e921332b2da99885cda111ad6b6e7d22965291fd2bc4ddd0199876ec5b2"
    );
    assert_eq!(result["output"]["stdout"], "");
    assert_eq!(result["output"]["stderr"], "");
    assert_eq!(fs::read(&fixture.model).unwrap(), CLEAN.as_bytes());
}

#[test]
fn human_task_output_renders_findings_and_coverage_from_the_shared_receipt() {
    let fixture = Fixture::new(CLEAN.as_bytes());
    let mut model = clean();
    model["panels"][0]["title"] = json!("synthetic\u{1b}[31m title");
    model["panels"][0]
        .as_object_mut()
        .unwrap()
        .remove("description");
    fixture.set(&model);
    let (_, result) = fixture.run();
    let output = Command::new(SAVE)
        .args(["task", "run", "dashboard-hygiene", "--input"])
        .arg(&fixture.input)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("task.run: succeeded\n"));
    assert!(text.contains("Checked panels: 1\nFindings: 1\n"));
    assert!(text.contains(result["data"]["findings"][0]["rule"].as_str().unwrap()));
    assert!(text.contains(result["data"]["findings"][0]["detail"].as_str().unwrap()));
    assert!(text.contains("Coverage: complete"));
    assert!(text.contains("no rendering, query parsing"));
    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains("binding_digest"));
    assert!(!text.contains("process_group_id"));
}

#[test]
fn each_upstream_rule_discriminates_its_single_field_mutation() {
    let fixture = Fixture::new(CLEAN.as_bytes());
    for rule in [
        "panel-title",
        "panel-description",
        "panel-units",
        "panel-no-targets",
        "panel-no-value",
        "panel-datasource",
        "target-rate-interval",
        "target-counter-agg",
        "template-all-value",
        "dashboard-tags",
    ] {
        let mut model = clean();
        match rule {
            "panel-title" => model["panels"][0]["title"] = json!("  "),
            "panel-description" => {
                model["panels"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("description");
            }
            "panel-units" => {
                model["panels"][0]["fieldConfig"]["defaults"]
                    .as_object_mut()
                    .unwrap()
                    .remove("unit");
            }
            "panel-no-targets" => model["panels"][0]["targets"] = json!([]),
            "panel-no-value" => {
                model["panels"][0]["fieldConfig"]["defaults"]
                    .as_object_mut()
                    .unwrap()
                    .remove("noValue");
            }
            "panel-datasource" => {
                model["panels"][0]["targets"][0]["datasource"] =
                    json!({"type":"prometheus","uid":"literal-uid"})
            }
            "target-rate-interval" => {
                model["panels"][0]["targets"][0]["expr"] = json!("rate(x_total[5m])")
            }
            "target-counter-agg" => {
                model["panels"][0]["targets"][0]["expr"] = json!("http_requests_total")
            }
            "template-all-value" => {
                model["templating"] =
                    json!({"list":[{"name":"job","includeAll":true,"allValue":""}]})
            }
            "dashboard-tags" => model["tags"] = json!([]),
            _ => unreachable!(),
        }
        fixture.set(&model);
        let (output, result) = fixture.run();
        assert_eq!(output.status.code(), Some(0), "{rule}: {result}");
        assert_eq!(result["execution"]["status"], "succeeded");
        assert_eq!(rules(&result), BTreeSet::from([rule]));
    }
    let output = fixture
        .command()
        .arg("--fail-on-findings")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(receipt(&output)["execution"]["status"], "succeeded");
}

#[test]
fn wrappers_nested_rows_and_nonquerying_panels_keep_upstream_semantics() {
    let fixture = Fixture::new(CLEAN.as_bytes());
    for model in [
        json!({"apiVersion":"dashboard.grafana.app/v1beta1","kind":"Dashboard","spec":clean()}),
        json!({"dashboard":clean(),"meta":{}}),
        {
            let mut model = clean();
            model["panels"] =
                json!([{"type":"row","panels":[{"type":"row","panels":model["panels"]}]}]);
            model
        },
        {
            let mut model = clean();
            model["panels"] = json!([{"id":9,"type":"text","title":"About","description":"How to read this dashboard"}]);
            model
        },
    ] {
        fixture.set(&model);
        let (output, result) = fixture.run();
        assert_eq!(output.status.code(), Some(0), "{result}");
        assert_eq!(result["data"]["checked_panels"], 1);
        assert!(rules(&result).is_empty());
    }
}

#[test]
fn dialect_overrides_quoted_literals_and_selected_range_totals_are_preserved() {
    let fixture = Fixture::new(CLEAN.as_bytes());
    for (dialect, expression, fired) in [
        ("loki", "rate({service=\"checkout\"}[$__interval])", false),
        ("prometheus", "rate(x_total[$__interval])", true),
        ("prometheus", "increase(x_total[$__range])", false),
        ("prometheus", "rate(x_total[$__range])", true),
        (
            "prometheus",
            "rate(x_total{label=~\"[a-z]+\",note=\"rate(fake_total[5m])\"}[$__rate_interval])",
            false,
        ),
        ("wavefront", "rate(x_total[5m])", false),
        ("splunk", "index=app | stats count", false),
    ] {
        let mut model = clean();
        model["panels"][0]["targets"][0]["datasource"] = json!({"type":dialect,"uid":"${source}"});
        model["panels"][0]["targets"][0]["expr"] = json!(expression);
        fixture.set(&model);
        let (_, result) = fixture.run();
        assert_eq!(
            rules(&result).contains("target-rate-interval"),
            fired,
            "{expression}: {result}"
        );
        assert!(!rules(&result).contains("target-counter-agg"));
    }
}

#[test]
fn malformed_empty_v2_and_wrongly_typed_models_fail_without_input_echo() {
    let fixture = Fixture::new(b"");
    for (bytes, code) in [
        (b"{".as_slice(), "invalid_model_json"),
        (b"\xff", "invalid_model_encoding"),
        (b"{\"panels\":[],\"panels\":[]}", "duplicate_model_key"),
        (b"{\"panels\":[]}", "uncheckable_model"),
        (b"{\"panels\":[{\"type\":\"row\"}]}", "uncheckable_model"),
        (b"{\"elements\":{},\"layout\":{}}", "unsupported_model_v2"),
        (b"{\"panels\":{}}", "invalid_model"),
        (
            b"{\"panels\":[{\"title\":\"synthetic-sensitive-marker\"}]}",
            "invalid_model",
        ),
    ] {
        fs::write(&fixture.model, bytes).unwrap();
        let (output, result) = fixture.run();
        assert_eq!(output.status.code(), Some(1), "{result}");
        assert!(error(&result, code), "{result}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-sensitive-marker"));
        assert_eq!(fs::read(&fixture.model).unwrap(), bytes);
    }
    for pointer in [
        "/panels/0/title",
        "/panels/0/description",
        "/panels/0/targets",
        "/panels/0/fieldConfig/defaults",
        "/panels/0/datasource/type",
    ] {
        let mut model = clean();
        *model.pointer_mut(pointer).unwrap() = json!(15);
        fixture.set(&model);
        let (_, result) = fixture.run();
        assert!(error(&result, "invalid_model"), "{pointer}: {result}");
    }
}

#[test]
fn model_byte_depth_panel_and_query_limits_fail_explicitly() {
    let fixture = Fixture::new(&vec![b' '; 2 * 1024 * 1024 + 1]);
    assert!(error(&fixture.run().1, "model_too_large"));
    let mut model = clean();
    model["panels"] = json!(vec![model["panels"][0].clone(); 1001]);
    fixture.set(&model);
    assert!(error(&fixture.run().1, "too_many_panels"));
    let mut model = clean();
    model["panels"][0]["targets"][0]["expr"] = json!("x".repeat(16001));
    fixture.set(&model);
    assert!(error(&fixture.run().1, "query_too_large"));
    let mut model = clean();
    let mut nested = json!(0);
    for _ in 0..33 {
        nested = json!([nested]);
    }
    model["unrelated"] = nested;
    fixture.set(&model);
    assert!(error(&fixture.run().1, "model_too_deep"));
}

#[test]
fn findings_fields_and_capture_loss_cannot_be_claimed_complete() {
    let fixture = Fixture::new(CLEAN.as_bytes());
    let mut model = clean();
    model["panels"][0]["title"] = json!("🦀".repeat(600));
    fixture.set(&model);
    let (output, result) = fixture.run();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(result["execution"]["status"], "partial");
    assert_eq!(result["data"]["fields_truncated"], true);
    assert!(error(&result, "task_findings_truncated"));
    let mut model = clean();
    model["panels"] = json!(vec![json!({"type":"stat","title":"Panel"}); 1000]);
    fixture.set(&model);
    let (output, result) = fixture.run();
    assert_eq!(result["execution"]["status"], "partial");
    assert_eq!(result["data"]["findings_truncated"], true);
    assert!(result["data"]["findings_total"].as_u64().unwrap() > 1000);
    assert!(output.stdout.len() <= 2 * 1024 * 1024);
    assert_eq!(result["output"]["stdout"], "");
    let output = fixture
        .command()
        .args(["--max-output-bytes", "1024"])
        .output()
        .unwrap();
    let result = receipt(&output);
    assert_eq!(result["execution"]["status"], "partial");
    assert!(error(&result, "task_output_incomplete"));
    assert!(error(&result, "output_truncated"));
    assert!(result["data"].get("findings").is_none());
}

#[test]
fn embedded_binding_works_away_from_checkout_and_ignores_hostile_modules() {
    let fixture = Fixture::new(CLEAN.as_bytes());
    let copied = fixture.directory.join("installed-save");
    // Keep the destination's writable fd in a separate process: other parallel test
    // forks must not inherit it before exec and transiently make this ELF ETXTBSY.
    assert!(
        Command::new("/usr/bin/cp")
            .arg(SAVE)
            .arg(&copied)
            .status()
            .unwrap()
            .success()
    );
    let marker = fixture.directory.join("poison-executed");
    fs::write(&fixture.input, json!({"file":"model.json"}).to_string()).unwrap();
    let poison = format!("raise RuntimeError({:?})\n", marker.to_str().unwrap());
    for name in [
        "dashboard_hygiene.py",
        "json.py",
        "hashlib.py",
        "sitecustomize.py",
        "usercustomize.py",
    ] {
        fs::write(fixture.directory.join(name), &poison).unwrap();
    }
    let output = Command::new(copied)
        .current_dir(&fixture.directory)
        .env("PYTHONPATH", &fixture.directory)
        .env("PYTHONHOME", &fixture.directory)
        .env("PYTHONSTARTUP", fixture.directory.join("sitecustomize.py"))
        .env("PATH", &fixture.directory)
        .args(["--json", "task", "run", "dashboard-hygiene", "--input"])
        .arg(&fixture.input)
        .output()
        .unwrap();
    let result = receipt(&output);
    assert_eq!(output.status.code(), Some(0), "{result}");
    assert!(rules(&result).is_empty());
    assert_eq!(
        result["data"]["input"]["file"],
        fixture.model.to_str().unwrap()
    );
    assert!(!marker.exists());
    assert!(!fixture.directory.join("__pycache__").exists());
}

#[test]
fn task_contract_rejects_override_unknown_id_version_and_relative_structured_path() {
    let fixture = Fixture::new(CLEAN.as_bytes());
    let base = json!({"spec_version":"0.1","request_id":"task-contract","operation":"task.run","operation_version":1,"target":{"kind":"local","id":"workstation"},"inputs":{"id":"dashboard-hygiene","version":1,"input":{"file":fixture.model}},"limits":{"timeout_ms":30000,"max_output_bytes":1048576},"record":"never"});
    for (pointer, replacement, code) in [
        ("/inputs/id", json!("other"), "unsupported_task"),
        ("/inputs/version", json!(2), "unsupported_task"),
        (
            "/inputs/input/file",
            json!("model.json"),
            "absolute_model_path_required",
        ),
    ] {
        let mut request = base.clone();
        *request.pointer_mut(pointer).unwrap() = replacement;
        fs::write(&fixture.input, request.to_string()).unwrap();
        let output = Command::new(SAVE)
            .args(["--json", "call", "--request"])
            .arg(&fixture.input)
            .output()
            .unwrap();
        let result = receipt(&output);
        assert_eq!(output.status.code(), Some(2));
        assert!(error(&result, code));
        assert_eq!(result["execution"]["child_exit_code"], Value::Null);
    }
    for key in ["runtime", "script", "digest", "environment", "role"] {
        let mut request = base.clone();
        request["inputs"][key] = json!("override");
        fs::write(&fixture.input, request.to_string()).unwrap();
        let output = Command::new(SAVE)
            .args(["--json", "call", "--request"])
            .arg(&fixture.input)
            .output()
            .unwrap();
        let result = receipt(&output);
        assert_eq!(output.status.code(), Some(2));
        assert!(error(&result, "invalid_task_input"));
        assert_eq!(result["execution"]["child_exit_code"], Value::Null);
    }
}

#[test]
fn nonregular_inputs_are_refused_without_blocking() {
    use std::ffi::CString;
    let fixture = Fixture::new(CLEAN.as_bytes());
    let fifo = fixture.directory.join("model.fifo");
    let name = CString::new(fifo.to_str().unwrap()).unwrap();
    // SAFETY: name is a NUL-terminated path inside this test's private directory.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    fs::write(&fixture.input, json!({"file":fifo}).to_string()).unwrap();
    let start = Instant::now();
    let (_, result) = fixture.run();
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(error(&result, "non_regular_model"));
}

#[test]
fn unknown_plugins_receive_common_findings_with_explicit_coverage_limits() {
    let fixture = Fixture::new(CLEAN.as_bytes());
    let mut model = clean();
    model["panels"][0]["type"] = json!("unknown-plugin-panel");
    model["panels"][0]
        .as_object_mut()
        .unwrap()
        .remove("description");
    fixture.set(&model);
    let (_, result) = fixture.run();
    assert_eq!(rules(&result), BTreeSet::from(["panel-description"]));
    assert!(
        result["coverage"]["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s
                .as_str()
                .unwrap()
                .contains("Unknown plugin panels receive common textual rules only"))
    );
    assert_eq!(result["assessment"], "not_assessed");
}

#[test]
fn task_timeout_and_cancellation_preserve_supervisor_outcomes() {
    let fixture = Fixture::new(CLEAN.as_bytes());
    let output = fixture
        .command()
        .args(["--timeout", "1ms"])
        .output()
        .unwrap();
    let result = receipt(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(result["execution"]["status"], "timed_out");
    assert!(error(&result, "timed_out"));
    assert!(result["data"].get("findings").is_none());
    let mut model = clean();
    let query = format!("{}x_total[5m]{}", "rate(".repeat(1400), ")".repeat(1400));
    model["panels"][0]["targets"] = json!(vec![json!({"refId":"A","expr":query}); 100]);
    fixture.set(&model);
    let mut child = fixture
        .command()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let children = format!("/proc/{}/task/{}/children", child.id(), child.id());
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if fs::read_to_string(&children).is_ok_and(|s| !s.trim().is_empty()) {
            break;
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("no supervised interpreter was observed");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    // SAFETY: child.id() is the live runner created by this test.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
    let output = child.wait_with_output().unwrap();
    let result = receipt(&output);
    assert_eq!(output.status.code(), Some(143));
    assert_eq!(result["execution"]["status"], "cancelled");
    assert_eq!(result["data"]["cancellation_signal"], 15);
    assert!(result["data"].get("findings").is_none());
}
