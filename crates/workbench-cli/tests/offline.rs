#![cfg(all(feature = "test-fixtures", target_os = "linux", target_arch = "x86_64"))]
#[allow(dead_code)]
#[path = "support/grafana_https.rs"]
mod https;

use https::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

const START: i64 = 1_790_899_200_000;

struct Mission {
    fixture: Fixture,
    bundle: Value,
}

impl Mission {
    fn new(missing: bool) -> Self {
        Self::scenario(missing, "normal")
    }

    fn scenario(missing: bool, scenario: &str) -> Self {
        let metric = json!({"schema":{"refId":"A","meta":{"type":"timeseries-multi","typeVersion":[0,1]},
            "fields":[{"name":"Time","type":"time","config":{"interval":1000}},
                {"name":"up","type":"number","labels":{"job":"fixture"},"config":{"unit":"none"}}]},
            "data":{"values":[[START,START+1000],[json!(1),if missing {Value::Null} else {json!(0)}]]}});
        let logs = json!({"schema":{"meta":{"typeVersion":[0,0],"custom":{"frameType":"LabeledTimeValues"}},
            "fields":[{"name":"labels","type":"other"},{"name":"Time","type":"time"},
                {"name":"Line","type":"string"},{"name":"tsNs","type":"string"},{"name":"id","type":"string"}]},
            "data":{"values":[[{"job":"fixture"},{"job":"fixture"},{"job":"fixture"}],
                [START,START,START],["duplicate","duplicate","$(touch /tmp/offline-command-must-not-run)"],
                ["1790899200000000123","1790899200000000123","1790899200000000123"],["a","b","c"]],
                "nanos":[null,[123,123,123],null,null,null]}});
        let mut replies = vec![
            Reply::org(),
            Reply::json(json!({"uid":"metrics-1","type":"prometheus","orgId":7})),
            Reply::json(json!({"results":{"A":{"status":200,"frames":[metric]}}})),
            Reply::org(),
            Reply::json(json!({"uid":"logs-1","type":"loki","orgId":7})),
            Reply::json(json!({"results":{"A":{"status":200,"frames":[logs]}}})),
        ];
        if scenario == "failed" {
            replies[5] = Reply::json(
                json!({"results":{"A":{"status":500,"error":"synthetic source failure"}}}),
            );
        } else if scenario == "incompatible" {
            replies[5] = Reply::json(
                json!({"results":{"A":{"status":200,"frames":[{"schema":{"fields":[]},"data":{"values":[]}}]}}}),
            );
        } else if scenario == "redacted" {
            let mut response: Value = serde_json::from_slice(&replies[5].body).unwrap();
            response["results"]["A"]["frames"][0]["data"]["values"][2][0] = json!("[REDACTED]");
            replies[5] = Reply::json(response);
        }
        let fixture = Fixture::new(replies);
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
        let output = fixture
            .command()
            .arg("--context-source")
            .arg(fixture.directory.join("context.json"))
            .args(["--context-max-age-days", "30", "--state-dir"])
            .arg(fixture.directory.join("state"))
            .args([
                "--record", "always", "evidence", "collect", "--target", "fixture", "--input",
            ])
            .arg(fixture.directory.join("input.json"))
            .output()
            .unwrap();
        let collection = receipt(&output);
        assert_eq!(
            output.status.code(),
            Some(if scenario == "failed" { 1 } else { 0 }),
            "{collection}"
        );
        assert_eq!(fixture.requests().len(), 6);
        // Remove the configuration and source after collection. Offline commands cannot
        // rely on either; no credential provider is inherited by the fresh process.
        fs::remove_file(&fixture.config).unwrap();
        fs::remove_file(fixture.directory.join("context.json")).unwrap();
        Self {
            fixture,
            bundle: collection["data"]["bundle"].clone(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .args(["--json", "--state-dir"])
            .arg(self.fixture.directory.join("state"));
        command
    }

    fn offline(&self, mode: &str) -> Command {
        let mut command = self.command();
        command.args([
            "evidence",
            mode,
            self.bundle["id"].as_str().unwrap(),
            "--expected-manifest-sha256",
            self.bundle["digest"].as_str().unwrap(),
        ]);
        command
    }

    fn run(&self, command: &mut Command, exit: i32) -> Value {
        let output = command.output().unwrap();
        let result = receipt(&output);
        assert_eq!(output.status.code(), Some(exit), "{result}");
        assert_eq!(result["assessment"], "not_assessed");
        assert_eq!(
            self.fixture.requests().len(),
            6,
            "Offline work contacted the old source"
        );
        assert!(!Path::new("/tmp/offline-command-must-not-run").exists());
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
        let mut out = BTreeMap::new();
        walk(&root, &root, &mut out);
        out
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
fn open_preserves_original_receipts_and_replay_is_deterministic_historical_analysis() {
    let mission = Mission::new(false);
    let snapshot = mission.snapshot();
    let opened = mission.run(&mut mission.offline("open"), 0);
    let replay = mission.run(&mut mission.offline("replay"), 0);
    let repeat = mission.run(&mut mission.offline("replay"), 0);
    assert_eq!(replay["data"]["checks"], repeat["data"]["checks"]);
    assert_eq!(replay["data"]["observation"], opened["data"]["observation"]);
    assert_eq!(
        replay["data"]["observation"]["window"]["from"],
        "2026-10-02T00:00:00Z"
    );
    assert_eq!(replay["data"]["analysis"]["profile"], "linux-offline-v1");
    assert_eq!(
        replay["data"]["algorithm"],
        json!({"id":"retained-observations-v1","version":1})
    );
    assert_eq!(
        replay["sources"][0]["observed_at"],
        opened["data"]["manifest"]["payload"]["started_at"]
    );
    assert_eq!(replay["sources"][0]["untrusted"], true);
    assert_eq!(check(&replay, "metrics")["summary"]["samples"], 2);
    assert_eq!(check(&replay, "metrics")["summary"]["finite"], 2);
    assert_eq!(
        check(&replay, "metrics")["details"][0]["timestamp_ns"],
        "1790899200000000000"
    );
    assert_eq!(check(&replay, "logs")["summary"]["entries"], 3);
    assert_eq!(check(&replay, "logs")["summary"]["unique_entries"], 2);
    assert_eq!(check(&replay, "logs")["summary"]["duplicate_entries"], 1);
    let duplicate = check(&replay, "logs")["details"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["line"] == "duplicate")
        .unwrap();
    assert_eq!(duplicate["count"], 2);
    assert_eq!(duplicate["timestamp_ns"], "1790899200000000123");
    for id in ["metrics", "logs"] {
        let original = &opened["data"]["manifest"]["artifacts"][id]["path"];
        let relative = format!(
            "runs/{}/{}",
            mission.bundle["id"].as_str().unwrap(),
            original.as_str().unwrap()
        );
        let stored: Value = serde_json::from_slice(&snapshot[&relative]).unwrap();
        assert_eq!(check(&opened, id)["details"][0]["receipt"], stored);
    }
    let path = mission.fixture.directory.join("request.json");
    fs::write(&path,json!({"spec_version":"0.1","request_id":"offline-parity","operation":"evidence.replay","operation_version":1,
        "target":{"kind":"local","id":"workstation"},"inputs":{"id":mission.bundle["id"],"expected_manifest_sha256":mission.bundle["digest"]},
        "limits":{"timeout_ms":30000,"max_output_bytes":1048576,"max_items":100},"record":"never"}).to_string()).unwrap();
    let structured = mission.run(mission.command().args(["call", "--request"]).arg(path), 0);
    assert_eq!(structured["data"]["checks"], replay["data"]["checks"]);
    assert_eq!(snapshot, mission.snapshot());
}

#[test]
fn detail_limits_disclose_omissions_without_changing_counts_or_evidence() {
    let mission = Mission::new(false);
    let snapshot = mission.snapshot();
    for mode in ["open", "replay"] {
        let limited = mission.run(mission.offline(mode).args(["--max-items", "1"]), 1);
        assert_eq!(limited["data"]["state"], "partial");
        assert!(limited["data"]["omitted_details"].as_u64().unwrap() > 0);
        assert_eq!(
            limited["data"]["checks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["details"].as_array().unwrap().len())
                .sum::<usize>(),
            1
        );
        if mode == "replay" {
            assert_eq!(check(&limited, "logs")["summary"]["entries"], 3);
        }
    }
    let byte_limited = mission.run(
        mission.offline("open").args(["--max-output-bytes", "1024"]),
        1,
    );
    assert_eq!(byte_limited["data"]["omitted_details"], 2);
    assert_eq!(snapshot, mission.snapshot());
}

#[test]
fn missing_samples_remain_unknown_and_tampered_artifacts_are_refused() {
    let mission = Mission::new(true);
    let partial = mission.run(&mut mission.offline("replay"), 1);
    assert_eq!(check(&partial, "metrics")["summary"]["missing"], 1);
    assert_eq!(check(&partial, "metrics")["summary"]["finite"], 1);
    assert_eq!(check(&partial, "metrics")["details"][1]["state"], "null");
    let opened = mission.run(&mut mission.offline("open"), 1);
    assert_eq!(check(&opened, "metrics")["state"], "partial");
    let artifact = mission
        .fixture
        .directory
        .join("state/runs")
        .join(mission.bundle["id"].as_str().unwrap())
        .join(
            opened["data"]["manifest"]["artifacts"]["metrics"]["path"]
                .as_str()
                .unwrap(),
        );
    let mut bytes = fs::read(&artifact).unwrap();
    bytes[0] = b'[';
    fs::write(&artifact, bytes).unwrap();
    for mode in ["open", "replay"] {
        let refused = mission.run(&mut mission.offline(mode), 1);
        assert_eq!(refused["execution"]["status"], "failed");
        assert!(refused["data"].get("checks").is_none());
    }
}

#[test]
fn manifest_pin_and_path_overrides_are_refused() {
    let mission = Mission::new(false);
    let snapshot = mission.snapshot();
    for mode in ["open", "replay"] {
        let mismatch = mission.run(
            mission.command().args([
                "evidence",
                mode,
                mission.bundle["id"].as_str().unwrap(),
                "--expected-manifest-sha256",
                &format!("sha256:{}", "0".repeat(64)),
            ]),
            1,
        );
        assert!(has_error(&mismatch, "manifest_digest_mismatch"));
        let denied = mission.run(
            mission.command().args(["evidence", mode, "../elsewhere"]),
            2,
        );
        assert!(has_error(&denied, "invalid_evidence_reference"));
    }
    assert_eq!(snapshot, mission.snapshot());
}

#[test]
fn failed_redacted_and_incompatible_observations_never_become_complete_replay() {
    for scenario in ["failed", "redacted", "incompatible"] {
        let mission = Mission::scenario(false, scenario);
        let snapshot = mission.snapshot();
        let result = mission.run(&mut mission.offline("replay"), 1);
        assert_eq!(result["data"]["state"], "partial");
        assert_eq!(
            check(&result, "logs")["state"],
            if scenario == "failed" {
                "missing"
            } else {
                "incompatible"
            }
        );
        assert!(
            check(&result, "logs")["details"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            !check(&result, "logs")["reasons"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let opened = mission.run(&mut mission.offline("open"), 1);
        assert_eq!(
            check(&opened, "logs")["state"],
            check(&result, "logs")["state"]
        );
        assert_eq!(
            check(&opened, "logs")["details"].as_array().unwrap().len(),
            1
        );
        if scenario == "failed" {
            assert_eq!(
                check(&opened, "logs")["details"][0]["receipt"]["execution"]["status"],
                "failed"
            );
        }
        assert_eq!(snapshot, mission.snapshot());
    }
}

#[test]
fn incomplete_bundle_is_refused_and_recovered_bundle_stays_limited() {
    let mission = Mission::new(false);
    let run = mission
        .fixture
        .directory
        .join("state/runs")
        .join(mission.bundle["id"].as_str().unwrap());
    fs::remove_file(run.join("manifest.json")).unwrap();
    let snapshot = mission.snapshot();
    for mode in ["open", "replay"] {
        let result = mission.run(
            mission
                .command()
                .args(["evidence", mode, mission.bundle["id"].as_str().unwrap()]),
            1,
        );
        assert!(has_error(&result, "store_incomplete"));
    }
    assert_eq!(snapshot, mission.snapshot());
    mission.run(mission.command().args(["evidence", "recover"]), 0);
    let recovered = mission.snapshot();
    for mode in ["open", "replay"] {
        let result = mission.run(
            mission
                .command()
                .args(["evidence", mode, mission.bundle["id"].as_str().unwrap()]),
            1,
        );
        assert_eq!(
            result["data"]["observation"]["manifest_state"],
            "interrupted"
        );
        assert!(
            result["data"]["source_limitations"]
                .as_array()
                .unwrap()
                .contains(&json!("interrupted_source_bundle"))
        );
    }
    assert_eq!(recovered, mission.snapshot());
}
