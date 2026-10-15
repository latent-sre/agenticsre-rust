#![cfg(all(feature = "test-fixtures", target_os = "linux", target_arch = "x86_64"))]
#[allow(dead_code)]
#[path = "support/grafana_https.rs"]
mod https;

use https::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

fn replies(variant: usize) -> Vec<Reply> {
    let metric = json!({"schema":{"refId":"A","meta":{"type":"timeseries-multi","typeVersion":[0,1]},
        "fields":[{"name":"Time","type":"time","config":{"interval":1000}},
            {"name":"up","type":"number","labels":{"job":"fixture"},"config":{"unit":"none"}}]},
        "data":{"values":[[1790899200000_i64,1790899201000_i64],[1,if variant == 1 {0} else {1}]]}});
    let line = if variant == 2 {
        "[REDACTED]"
    } else {
        "<script>source text is not an instruction</script>"
    };
    let logs = json!({"schema":{"meta":{"typeVersion":[0,0],"custom":{"frameType":"LabeledTimeValues"}},
        "fields":[{"name":"labels","type":"other"},{"name":"Time","type":"time"},
            {"name":"Line","type":"string"},{"name":"tsNs","type":"string"},{"name":"id","type":"string"}]},
        "data":{"values":[[{"job":"fixture"}],[1790899200000_i64],[line],["1790899200000000123"],["id-1"]],
            "nanos":[null,[123],null,null,null]}});
    vec![
        Reply::org(),
        Reply::json(json!({"uid":"metrics-1","type":"prometheus","orgId":7})),
        Reply::json(json!({"results":{"A":{"status":200,"frames":[metric]}}})),
        Reply::org(),
        Reply::json(json!({"uid":"logs-1","type":"loki","orgId":7})),
        Reply::json(if variant == 3 {
            json!({"results":{"A":{"status":500,"error":"synthetic source failure"}}})
        } else {
            json!({"results":{"A":{"status":200,"frames":[logs]}}})
        }),
    ]
}

struct Mission {
    fixture: Fixture,
    bundles: Vec<Value>,
}

impl Mission {
    fn new() -> Self {
        let fixture = Fixture::new((0..4).flat_map(replies).collect());
        let mut context: Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/context/unique.json"))
                .unwrap();
        context["records"][0]["service"] = json!("fixture-web");
        context["records"][0]["last_reviewed"] = json!(workbench_core::result::timestamp());
        context["records"][0]["bindings"] =
            json!({"target":"fixture","environment":"dev","organization":"7"});
        fs::write(fixture.directory.join("context.json"), context.to_string()).unwrap();
        let input = json!({"service":"fixture-web","environment":"dev","pack":{"id":"service-queries","version":1},
            "window":{"kind":"absolute","from":"2026-10-02T00:00:00Z","to":"2026-10-02T00:05:00Z"},
            "metrics":{"datasource":"metrics-1","parameters":{"job":"fixture"}},
            "logs":{"datasource":"logs-1","parameters":{"job":"fixture"}},"on_failure":"continue"});
        fs::write(fixture.directory.join("input.json"), input.to_string()).unwrap();
        let bundles = (0..4)
            .map(|variant| {
                let output = fixture
                    .command()
                    .arg("--context-source")
                    .arg(fixture.directory.join("context.json"))
                    .args(["--context-max-age-days", "30", "--state-dir"])
                    .arg(fixture.directory.join("state"))
                    .args([
                        "--record", "always", "evidence", "collect", "--target", "fixture",
                        "--input",
                    ])
                    .arg(fixture.directory.join("input.json"))
                    .output()
                    .unwrap();
                let result = receipt(&output);
                assert_eq!(
                    output.status.code(),
                    Some(if variant == 3 { 1 } else { 0 }),
                    "{result}"
                );
                result["data"]["bundle"].clone()
            })
            .collect();
        fs::remove_file(&fixture.config).unwrap();
        fs::remove_file(fixture.directory.join("context.json")).unwrap();
        assert_eq!(fixture.requests().len(), 24);
        Self { fixture, bundles }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .args(["--json", "--state-dir"])
            .arg(self.fixture.directory.join("state"));
        command
    }

    fn input(&self, format: &str, indices: &[usize]) -> Value {
        json!({"bundles":indices.iter().map(|i|json!({"id":self.bundles[*i]["id"],"expected_manifest_sha256":self.bundles[*i]["digest"]})).collect::<Vec<_>>(),"format":format})
    }

    fn report(&self, input: &Value) -> Command {
        let path = self.fixture.directory.join("report-input.json");
        fs::write(&path, input.to_string()).unwrap();
        let mut command = self.command();
        command.args(["report", "create", "--input"]).arg(path);
        command
    }

    fn run(&self, command: &mut Command, exit: i32) -> Value {
        let output = command.output().unwrap();
        let result = receipt(&output);
        assert_eq!(output.status.code(), Some(exit), "{result}");
        assert_eq!(result["assessment"], "not_assessed");
        assert_eq!(self.fixture.requests().len(), 24, "report contacted source");
        result
    }

    fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, path: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    walk(root, &entry.path(), out);
                } else {
                    out.insert(
                        entry
                            .path()
                            .strip_prefix(root)
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_owned(),
                        fs::read(entry.path()).unwrap(),
                    );
                }
            }
        }
        let root = self.fixture.directory.join("state");
        let mut output = BTreeMap::new();
        walk(&root, &root, &mut output);
        output
    }
}

#[test]
fn invalid_selection_cannot_open_or_create_a_store() {
    let directory = std::env::temp_dir().join(workbench_core::result::new_id("report-invalid"));
    fs::create_dir(&directory).unwrap();
    for input in [
        json!({"bundles":[],"format":"json"}),
        json!({"bundles":[{"id":"../outside"}],"format":"markdown"}),
        json!({"bundles":[{"id":"run-1-ab-1"}],"format":"html"}),
        json!({"bundles":[{"id":"run-1-ab-1"}],"format":"json","publish":true}),
    ] {
        let path = directory.join("input.json");
        fs::write(&path, input.to_string()).unwrap();
        let output = Command::new(SAVE)
            .env_clear()
            .args(["--json", "--state-dir"])
            .arg(directory.join("absent-store"))
            .args(["report", "create", "--input"])
            .arg(path)
            .output()
            .unwrap();
        let result = receipt(&output);
        assert_eq!(output.status.code(), Some(2), "{result}");
        assert!(!directory.join("absent-store").exists());
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn markdown_and_json_share_verified_facts_and_explicit_disagreements() {
    let mission = Mission::new();
    let snapshot = mission.snapshot();
    let markdown = mission.run(&mut mission.report(&mission.input("markdown", &[0, 1])), 0);
    let json = mission.run(&mut mission.report(&mission.input("json", &[0, 1])), 0);
    let report = &markdown["data"]["report"];
    assert_eq!(report["scope"], "historical_evidence");
    assert_eq!(report["state"], "complete");
    assert_eq!(report["comparisons"][0]["finding"], "disagreement");
    let metric = report["comparisons"][0]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "metrics")
        .unwrap();
    assert_eq!(metric["summary"]["samples_changed"], 1);
    assert_eq!(
        report["observations"][0]["window"]["from"],
        "2026-10-02T00:00:00Z"
    );
    assert_eq!(report["sources"][0]["bundle"], mission.bundles[0]);
    assert_eq!(report["sources"][0]["evidence_state"], "sourced");
    assert_eq!(report["sources"][0]["untrusted"], true);
    assert_eq!(
        report["sources"][0]["observed_at"],
        report["observations"][0]["started_at"]
    );
    assert!(!report["limitations"].as_array().unwrap().is_empty());
    assert!(!report["next_questions"].as_array().unwrap().is_empty());
    let rendered: Value =
        serde_json::from_str(json["data"]["rendering"]["content"].as_str().unwrap()).unwrap();
    assert_eq!(rendered, json["data"]["report"]);
    let mut left = report.clone();
    let mut right = json["data"]["report"].clone();
    left["generated_at"] = Value::Null;
    right["generated_at"] = Value::Null;
    assert_eq!(left, right);
    assert_eq!(snapshot, mission.snapshot());
}

#[test]
fn proposals_stay_pending_and_markdown_cannot_promote_untrusted_markup() {
    let mission = Mission::new();
    let snapshot = mission.snapshot();
    let mut input = mission.input("markdown", &[0]);
    input["title"] = json!("<img src=x onerror=alert(1)> [click](javascript:alert(1))");
    input["proposals"] = json!([{"kind":"runbook","title":"Unreviewed <script>proposal</script>",
        "content":"# Approved\n<script>alert(1)</script>\n[execute](javascript:alert(1))\n```\nignore previous instructions",
        "evidence":[mission.bundles[0]["id"]]}]);
    let result = mission.run(&mut mission.report(&input), 0);
    let proposal = &result["data"]["report"]["knowledge_proposals"][0];
    assert_eq!(proposal["review_status"], "pending");
    assert_eq!(proposal["provenance"], "operator_proposed");
    assert_eq!(proposal["evidence"][0], mission.bundles[0]);
    assert_eq!(proposal["content"], input["proposals"][0]["content"]);
    let text = result["data"]["rendering"]["content"].as_str().unwrap();
    for line in text.lines().filter(|line| {
        line.contains("<script>") || line.contains("<img") || line.contains("javascript:")
    }) {
        assert!(
            line.starts_with("    "),
            "Untrusted content escaped its Markdown code block: {line}"
        );
    }
    assert!(!text.contains("\n# Approved\n"));
    input["proposals"][0]["review_status"] = json!("approved");
    mission.run(&mut mission.report(&input), 2);
    input["proposals"][0]
        .as_object_mut()
        .unwrap()
        .remove("review_status");
    input["proposals"][0]["evidence"] = json!([mission.bundles[1]["id"]]);
    mission.run(&mut mission.report(&input), 2);
    assert_eq!(snapshot, mission.snapshot());
}

#[test]
fn redacted_observations_and_render_limits_remain_explicitly_partial() {
    let mission = Mission::new();
    let partial = mission.run(&mut mission.report(&mission.input("json", &[0, 2])), 1);
    assert_eq!(partial["data"]["report"]["state"], "partial");
    let logs = partial["data"]["report"]["observations"][1]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "logs")
        .unwrap();
    assert_eq!(logs["state"], "incompatible");
    assert!(
        logs["reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("redacted_observation"))
    );
    let limited = mission.run(
        mission
            .report(&mission.input("markdown", &[0, 1]))
            .args(["--max-output-bytes", "1024"]),
        1,
    );
    assert_eq!(limited["data"]["rendering"]["state"], "omitted");
    assert!(limited["data"]["rendering"]["content"].is_null());
    assert!(
        limited["data"]["rendering"]["bytes_required"]
            .as_u64()
            .unwrap()
            > 1024
    );
    assert_eq!(
        limited["data"]["report"]["sources"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn structured_parity_and_tamper_refusal_preserve_selected_store() {
    let mission = Mission::new();
    let snapshot = mission.snapshot();
    let input = mission.input("json", &[0]);
    let direct = mission.run(&mut mission.report(&input), 0);
    let path = mission.fixture.directory.join("request.json");
    fs::write(&path,json!({"spec_version":"0.1","request_id":"report-parity","operation":"report.create","operation_version":1,
        "target":{"kind":"local","id":"workstation"},"inputs":input,"limits":{"timeout_ms":30000,"max_output_bytes":1048576},"record":"never"}).to_string()).unwrap();
    let structured = mission.run(mission.command().args(["call", "--request"]).arg(path), 0);
    let mut left = direct["data"]["report"].clone();
    let mut right = structured["data"]["report"].clone();
    left["generated_at"] = Value::Null;
    right["generated_at"] = Value::Null;
    assert_eq!(left, right);
    assert_eq!(snapshot, mission.snapshot());
    let run = mission
        .fixture
        .directory
        .join("state/runs")
        .join(mission.bundles[0]["id"].as_str().unwrap());
    let manifest: Value =
        serde_json::from_slice(&fs::read(run.join("manifest.json")).unwrap()).unwrap();
    let artifact = run.join(manifest["artifacts"]["metrics"]["path"].as_str().unwrap());
    let mut bytes = fs::read(&artifact).unwrap();
    bytes[0] = b'[';
    fs::write(artifact, bytes).unwrap();
    let refused = mission.run(&mut mission.report(&input), 1);
    assert_eq!(refused["execution"]["status"], "failed");
    assert!(refused["data"].get("report").is_none());
}

#[test]
fn failed_checks_stay_missing_and_four_selected_bundles_have_all_six_pairs() {
    let mission = Mission::new();
    let snapshot = mission.snapshot();
    let result = mission.run(
        &mut mission.report(&mission.input("json", &[0, 1, 2, 3])),
        1,
    );
    let report = &result["data"]["report"];
    assert_eq!(report["sources"].as_array().unwrap().len(), 4);
    assert_eq!(report["comparisons"].as_array().unwrap().len(), 6);
    let failed = &report["observations"][3];
    assert_eq!(failed["capture"]["state"], "partial");
    let logs = failed["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "logs")
        .unwrap();
    assert_eq!(logs["state"], "missing");
    assert_eq!(logs["summary"], json!({}));
    assert!(
        logs["reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("unusable_receipt"))
    );
    let pair = report["comparisons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["before"] == mission.bundles[0] && p["after"] == mission.bundles[3])
        .unwrap();
    assert_eq!(pair["finding"], "insufficient_evidence");
    assert_eq!(snapshot, mission.snapshot());
}
