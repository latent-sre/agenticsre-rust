#![cfg(all(feature = "test-fixtures", target_os = "linux"))]
#[allow(dead_code)]
#[path = "support/grafana_https.rs"]
mod https;

use https::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

const START: i64 = 1_790_899_200_000;
const CONTEXT: &str = include_str!("../../../tests/fixtures/context/unique.json");

fn input() -> Value {
    json!({"service":"fixture-web","environment":"dev",
        "pack":{"id":"service-queries","version":1},
        "window":{"kind":"absolute","from":"2026-10-02T00:00:00Z","to":"2026-10-02T00:05:00Z"},
        "metrics":{"datasource":"metrics-1","parameters":{"job":"fixture"}},
        "logs":{"datasource":"logs-1","parameters":{"job":"fixture"}},"on_failure":"continue"})
}

fn metric(instance: &str, shift: i64, values: Value) -> Value {
    json!({"schema":{"refId":"A","meta":{"type":"timeseries-multi","typeVersion":[0,1]},
        "fields":[{"name":"Time","type":"time","config":{"interval":1000}},
            {"name":"up","type":"number","labels":{"job":"fixture","instance":instance},"config":{"unit":"none"}}]},
        "data":{"values":[[START+shift,START+shift+1000],values]}})
}

fn logs(shift: i64, lines: &[&str]) -> Value {
    // Equal timestamps deliberately preserve duplicate multiplicity. Nanoseconds
    // distinguish this observation from a millisecond-rounded log entry.
    let times: Vec<_> = lines.iter().map(|_| START + shift).collect();
    let labels: Vec<_> = lines.iter().map(|_| json!({"job":"fixture"})).collect();
    let nanos: Vec<_> = lines.iter().map(|_| 123).collect();
    let precise: Vec<_> = lines
        .iter()
        .map(|_| ((START + shift) as i128 * 1_000_000 + 123).to_string())
        .collect();
    let ids: Vec<_> = lines
        .iter()
        .enumerate()
        .map(|(i, _)| format!("generated-{i}"))
        .collect();
    json!({"schema":{"meta":{"typeVersion":[0,0],"custom":{"frameType":"LabeledTimeValues"}},
        "fields":[{"name":"labels","type":"other"},{"name":"Time","type":"time"},
            {"name":"Line","type":"string"},{"name":"tsNs","type":"string"},{"name":"id","type":"string"}]},
        "data":{"values":[labels,times,lines,precise,ids],"nanos":[null,nanos,null,null,null]}})
}

fn query(frames: Vec<Value>) -> Reply {
    Reply::json(json!({"results":{"A":{"status":200,"frames":frames}}}))
}

fn collection_replies(metrics: Vec<Value>, logs: Value) -> Vec<Reply> {
    vec![
        Reply::org(),
        Reply::json(json!({"uid":"metrics-1","type":"prometheus","orgId":7})),
        query(metrics),
        Reply::org(),
        Reply::json(json!({"uid":"logs-1","type":"loki","orgId":7})),
        query(vec![logs]),
    ]
}

struct Mission {
    https: Fixture,
}

impl Mission {
    fn new(replies: Vec<Reply>) -> Self {
        let https = Fixture::new(replies);
        let mut context: Value = serde_json::from_str(CONTEXT).unwrap();
        context["records"][0]["service"] = json!("fixture-web");
        context["records"][0]["last_reviewed"] = json!(workbench_core::result::timestamp());
        context["records"][0]["bindings"] =
            json!({"target":"fixture","environment":"dev","organization":"7"});
        fs::write(https.directory.join("context.json"), context.to_string()).unwrap();
        Self { https }
    }

    fn collect(&self, inputs: &Value, exit: i32) -> Value {
        let path = self.https.directory.join("input.json");
        fs::write(&path, inputs.to_string()).unwrap();
        let mut command = self.https.command();
        command
            .arg("--context-source")
            .arg(self.https.directory.join("context.json"))
            .args(["--context-max-age-days", "30"])
            .arg("--state-dir")
            .arg(self.https.directory.join("state"))
            .args([
                "--record", "always", "evidence", "collect", "--target", "fixture", "--input",
            ])
            .arg(path);
        self.run(&mut command, exit)["data"]["bundle"].clone()
    }

    fn local(&self) -> Command {
        // Comparison runs in a new process with no provider, context, or Grafana
        // configuration. The server's request count proves no replay occurs.
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .args(["--json", "--state-dir"])
            .arg(self.https.directory.join("state"));
        command
    }

    fn compare(&self, before: &Value, after: &Value, policy: &str) -> Command {
        let mut command = self.local();
        command.args([
            "evidence",
            "compare",
            before["id"].as_str().unwrap(),
            after["id"].as_str().unwrap(),
            "--normalization",
            policy,
        ]);
        command
    }

    fn run(&self, command: &mut Command, exit: i32) -> Value {
        let (output, result) = self.https.run(command);
        assert_eq!(output.status.code(), Some(exit), "{result}");
        assert_eq!(result["assessment"], "not_assessed");
        result
    }

    fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        fn visit(root: &Path, path: &Path, output: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    visit(root, &entry.path(), output);
                } else {
                    output.insert(
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
        let root = self.https.directory.join("state");
        let mut output = BTreeMap::new();
        visit(&root, &root, &mut output);
        output
    }
}

fn check<'a>(result: &'a Value, id: &str) -> &'a Value {
    result["data"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == id)
        .unwrap()
}

#[test]
fn public_comparison_counts_changes_and_preserves_store_without_external_access() {
    let mut replies = collection_replies(
        vec![
            metric("a", 0, json!([1, 1])),
            metric("removed", 0, json!([1, 0])),
        ],
        logs(0, &["duplicate", "duplicate", "removed"]),
    );
    replies.extend(collection_replies(
        vec![
            metric("a", 0, json!([1, 0])),
            metric("added", 0, json!([0, 1])),
        ],
        logs(0, &["duplicate", "added"]),
    ));
    let mission = Mission::new(replies);
    let before = mission.collect(&input(), 0);
    let after = mission.collect(&input(), 0);
    let snapshot = mission.snapshot();
    let direct = mission.run(&mut mission.compare(&before, &after, "exact-window-v1"), 0);
    assert_eq!(direct["data"]["state"], "complete");
    assert_eq!(direct["data"]["scope"], "returned_observations");
    assert_eq!(direct["data"]["algorithm"], "returned-observations-v1");
    for (index, side) in ["before", "after"].into_iter().enumerate() {
        let source = &direct["sources"][index];
        let bundle = &direct["data"][side]["bundle"];
        assert_eq!(source.as_object().unwrap().len(), 4);
        assert_eq!(
            source["reference"],
            format!(
                "evidence-bundle:{}@{}",
                bundle["id"].as_str().unwrap(),
                bundle["digest"].as_str().unwrap()
            )
        );
        assert_eq!(source["observed_at"], direct["data"][side]["started_at"]);
        assert_eq!(source["evidence_state"], "sourced");
        assert_eq!(source["untrusted"], true);
    }
    for (name, expected) in [
        ("series_added", 1),
        ("series_removed", 1),
        ("samples_changed", 1),
        ("samples_unchanged", 1),
        ("samples_added", 2),
        ("samples_removed", 2),
    ] {
        assert_eq!(
            check(&direct, "metrics")["summary"][name],
            expected,
            "{name}: {direct}"
        );
    }
    for (name, expected) in [
        ("entries_added", 1),
        ("entries_removed", 2),
        ("entries_unchanged", 1),
    ] {
        assert_eq!(check(&direct, "logs")["summary"][name], expected);
    }
    let duplicate = check(&direct, "logs")["details"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["line"] == "duplicate")
        .unwrap();
    assert_eq!(duplicate["count"], 1);
    assert_eq!(duplicate["timestamp_ns"], "1790899200000000123");
    let request = json!({"spec_version":"0.1","request_id":"comparison-parity","operation":"evidence.compare","operation_version":1,
        "target":{"kind":"local","id":"workstation"},
        "inputs":{"before":{"id":before["id"]},"after":{"id":after["id"]},"normalization":"exact-window-v1"},
        "limits":{"timeout_ms":30000,"max_output_bytes":1048576,"max_items":100},"record":"never"});
    let path = mission.https.directory.join("request.json");
    fs::write(&path, request.to_string()).unwrap();
    let structured = mission.run(mission.local().args(["call", "--request"]).arg(path), 0);
    assert_eq!(structured["data"], direct["data"]);
    let equal = mission.run(&mut mission.compare(&before, &before, "exact-window-v1"), 0);
    assert_eq!(check(&equal, "metrics")["summary"]["samples_unchanged"], 4);
    assert_eq!(check(&equal, "logs")["summary"]["entries_unchanged"], 3);
    assert_eq!(mission.snapshot(), snapshot);
    assert_eq!(mission.https.requests().len(), 12);
}

#[test]
fn offset_policy_aligns_equal_duration_windows_and_preserves_original_times() {
    let mut replies = collection_replies(vec![metric("a", 0, json!([1, 1]))], logs(0, &["before"]));
    replies.extend(collection_replies(
        vec![metric("a", 300_000, json!([1, 0]))],
        logs(300_000, &["after"]),
    ));
    let mission = Mission::new(replies);
    let before = mission.collect(&input(), 0);
    let mut shifted = input();
    shifted["window"]["from"] = json!("2026-10-02T00:05:00Z");
    shifted["window"]["to"] = json!("2026-10-02T00:10:00Z");
    let after = mission.collect(&shifted, 0);
    let refused = mission.run(&mut mission.compare(&before, &after, "exact-window-v1"), 1);
    assert_eq!(refused["data"]["compatibility"]["state"], "incompatible");
    let offset = mission.run(&mut mission.compare(&before, &after, "window-offset-v1"), 0);
    assert_eq!(check(&offset, "metrics")["summary"]["samples_changed"], 1);
    let detail = &check(&offset, "metrics")["details"][0];
    assert_eq!(detail["timestamp_ns"], "1000000000");
    assert_eq!(detail["before_timestamp_ns"], "1790899201000000000");
    assert_eq!(detail["after_timestamp_ns"], "1790899501000000000");
    assert_ne!(
        offset["data"]["before"]["window"],
        offset["data"]["after"]["window"]
    );
    assert_eq!(mission.https.requests().len(), 12);
}

#[test]
fn typed_frame_failures_and_nulls_never_become_complete_comparisons() {
    let base = metric("a", 0, json!([1, 1]));
    let mut cases = Vec::new();
    let mut units = base.clone();
    units["schema"]["fields"][1]["config"]["unit"] = json!("percent");
    cases.push((units, "incompatible_units"));
    let mut sampling = base.clone();
    sampling["schema"]["fields"][0]["config"]["interval"] = json!(2000);
    cases.push((sampling, "incompatible_sampling"));
    let mut shape = base.clone();
    shape["schema"]["meta"]["type"] = json!("timeseries-long");
    cases.push((shape, "unsupported_frame_profile"));
    let mut redacted = base.clone();
    redacted["schema"]["fields"][1]["labels"]["instance"] = json!(TOKEN);
    cases.push((redacted, "redacted_observation"));
    cases.push((metric("a", 0, json!([1, null])), "missing_samples"));
    let mut replies = collection_replies(vec![base], logs(0, &["same"]));
    for (frame, _) in &cases {
        replies.extend(collection_replies(vec![frame.clone()], logs(0, &["same"])));
    }
    let mission = Mission::new(replies);
    let before = mission.collect(&input(), 0);
    for (_, reason) in cases {
        let after = mission.collect(&input(), 0);
        let result = mission.run(&mut mission.compare(&before, &after, "exact-window-v1"), 1);
        assert_ne!(result["data"]["state"], "complete");
        assert!(
            check(&result, "metrics")["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == reason),
            "{reason}: {result}"
        );
        assert_eq!(check(&result, "logs")["summary"]["entries_unchanged"], 1);
    }
    assert_eq!(mission.https.requests().len(), 36);
}

#[test]
fn partial_collection_keeps_missing_check_visible() {
    let mut replies = collection_replies(vec![metric("a", 0, json!([1, 1]))], logs(0, &["same"]));
    let mut failed = collection_replies(vec![metric("a", 0, json!([1, 1]))], logs(0, &["same"]));
    failed[5] =
        Reply::json(json!({"results":{"A":{"error":"fixture query unavailable","status":500}}}));
    replies.extend(failed);
    let mission = Mission::new(replies);
    let before = mission.collect(&input(), 0);
    let after = mission.collect(&input(), 1);
    assert!(after.is_object());
    let result = mission.run(&mut mission.compare(&before, &after, "exact-window-v1"), 1);
    assert_eq!(result["data"]["state"], "partial");
    assert_eq!(check(&result, "logs")["state"], "missing");
    assert_eq!(check(&result, "metrics")["summary"]["samples_unchanged"], 2);
}

#[test]
fn item_and_byte_limits_disclose_omissions_and_manifest_pins_are_checked() {
    let long_old = "old log entry ".repeat(150);
    let long_new = "new log entry ".repeat(150);
    let mut replies =
        collection_replies(vec![metric("a", 0, json!([1, 1]))], logs(0, &[&long_old]));
    replies.extend(collection_replies(
        vec![metric("a", 0, json!([0, 0]))],
        logs(0, &[&long_new]),
    ));
    let mission = Mission::new(replies);
    let before = mission.collect(&input(), 0);
    let after = mission.collect(&input(), 0);
    let snapshot = mission.snapshot();
    for options in [["--max-items", "1"], ["--max-output-bytes", "1024"]] {
        let result = mission.run(
            mission
                .compare(&before, &after, "exact-window-v1")
                .args(options),
            1,
        );
        assert_eq!(result["data"]["state"], "partial");
        assert!(
            result["data"]["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["omitted_details"].as_u64().unwrap_or(0) > 0)
        );
        assert_eq!(check(&result, "metrics")["summary"]["samples_changed"], 2);
        assert!(serde_json::to_vec(&result).unwrap().len() < 2 * 1024 * 1024);
    }
    let mismatch = format!("sha256:{}", "0".repeat(64));
    let result = mission.run(
        mission
            .compare(&before, &after, "exact-window-v1")
            .args(["--before-manifest-sha256", &mismatch]),
        1,
    );
    assert!(!result["errors"].as_array().unwrap().is_empty());
    assert_eq!(mission.snapshot(), snapshot);
    assert_eq!(mission.https.requests().len(), 12);
}

#[test]
fn byte_budget_preserves_one_global_detail_prefix_and_counts_empty_arrays() {
    let instance = "instance-".repeat(160);
    let mut replies =
        collection_replies(vec![metric(&instance, 0, json!([1, 1]))], logs(0, &["old"]));
    replies.extend(collection_replies(
        vec![metric(&instance, 0, json!([0, 1]))],
        logs(0, &["new"]),
    ));
    let mission = Mission::new(replies);
    let before = mission.collect(&input(), 0);
    let after = mission.collect(&input(), 0);
    let complete = mission.run(&mut mission.compare(&before, &after, "exact-window-v1"), 0);
    let metric_bytes = serde_json::to_vec(&check(&complete, "metrics")["details"])
        .unwrap()
        .len();
    assert!(metric_bytes > 1024);
    for budget in [1024, metric_bytes] {
        let result = mission.run(
            mission
                .compare(&before, &after, "exact-window-v1")
                .args(["--max-output-bytes", &budget.to_string()]),
            1,
        );
        let metrics = check(&result, "metrics");
        let logs = check(&result, "logs");
        // The metric change is first in the canonical detail stream. Once it
        // cannot fit, a smaller later log change must not restart the prefix.
        // Even an empty second array consumes two encoded bytes.
        assert_eq!(metrics["details"], json!([]), "budget {budget}: {result}");
        assert_eq!(logs["details"], json!([]), "budget {budget}: {result}");
        assert_eq!(metrics["omitted_details"], 1);
        assert_eq!(logs["omitted_details"], 2);
        assert_eq!(metrics["summary"]["samples_changed"], 1);
        assert_eq!(metrics["summary"]["samples_unchanged"], 1);
        assert_eq!(logs["summary"]["entries_added"], 1);
        assert_eq!(logs["summary"]["entries_removed"], 1);
        let encoded_arrays: usize = [metrics, logs]
            .iter()
            .map(|check| serde_json::to_vec(&check["details"]).unwrap().len())
            .sum();
        assert!(encoded_arrays <= budget, "{encoded_arrays} > {budget}");
    }
    assert_eq!(mission.https.requests().len(), 12);
}

#[test]
fn parameter_identity_changes_refuse_observation_diff() {
    let mut replies = collection_replies(vec![metric("a", 0, json!([1, 1]))], logs(0, &["same"]));
    replies.extend(collection_replies(
        vec![metric("a", 0, json!([1, 1]))],
        logs(0, &["same"]),
    ));
    let mission = Mission::new(replies);
    let before = mission.collect(&input(), 0);
    let mut changed = input();
    changed["metrics"]["parameters"]["job"] = json!("different-job");
    let after = mission.collect(&changed, 0);
    let result = mission.run(&mut mission.compare(&before, &after, "exact-window-v1"), 1);
    assert_eq!(check(&result, "metrics")["state"], "incompatible");
    assert_eq!(mission.https.requests().len(), 12);
}

#[test]
fn datasource_and_service_identity_changes_are_explicitly_incompatible() {
    let ordinary = || collection_replies(vec![metric("a", 0, json!([1, 1]))], logs(0, &["same"]));
    let mut replies = ordinary();
    let mut changed_datasource = ordinary();
    changed_datasource[1] = Reply::json(json!({"uid":"metrics-2","type":"prometheus","orgId":7}));
    replies.extend(changed_datasource);
    replies.extend(ordinary());
    let mission = Mission::new(replies);
    let before = mission.collect(&input(), 0);
    let mut changed = input();
    changed["metrics"]["datasource"] = json!("metrics-2");
    let other_datasource = mission.collect(&changed, 0);
    let result = mission.run(
        &mut mission.compare(&before, &other_datasource, "exact-window-v1"),
        1,
    );
    assert_eq!(check(&result, "metrics")["state"], "incompatible");
    // Change the authoritative context before collecting a different service;
    // never forge the retained bundle to create an identity mismatch.
    let source = mission.https.directory.join("context.json");
    let mut context: Value = serde_json::from_slice(&fs::read(&source).unwrap()).unwrap();
    context["records"][0]["service"] = json!("another-service");
    fs::write(source, context.to_string()).unwrap();
    let mut changed = input();
    changed["service"] = json!("another-service");
    let other_service = mission.collect(&changed, 0);
    let result = mission.run(
        &mut mission.compare(&before, &other_service, "exact-window-v1"),
        1,
    );
    assert_eq!(result["data"]["compatibility"]["state"], "incompatible");
    assert_eq!(result["data"]["state"], "incompatible");
    assert_eq!(mission.https.requests().len(), 18);
}

#[test]
fn malformed_references_and_policy_are_denied_before_store_access() {
    let mission = Mission::new(vec![]);
    for (inputs, code) in [
        (
            json!({"before":{"id":"../../outside"},"after":{"id":"../../outside"},"normalization":"exact-window-v1"}),
            "invalid_evidence_reference",
        ),
        (
            json!({"before":{"id":"run-1-ab-c"},"after":{"id":"run-1-ab-c"},"normalization":"guess-v1"}),
            "invalid_comparison_input",
        ),
    ] {
        let path = mission.https.directory.join("bad-request.json");
        fs::write(&path,json!({"spec_version":"0.1","request_id":"bad-comparison","operation":"evidence.compare","operation_version":1,
            "target":{"kind":"local","id":"workstation"},
            "inputs":inputs,"limits":{"timeout_ms":30000,"max_output_bytes":1048576},"record":"never"}).to_string()).unwrap();
        let result = mission.run(mission.local().args(["call", "--request"]).arg(path), 2);
        assert!(has_error(&result, code), "{result}");
    }
    assert!(!mission.https.directory.join("state").exists());
    assert!(mission.https.requests().is_empty());
}
