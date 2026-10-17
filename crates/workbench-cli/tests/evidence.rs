#![cfg(all(feature = "test-fixtures", target_os = "linux"))]
#[allow(dead_code)]
#[path = "support/grafana_https.rs"]
mod https;

use https::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const CONTEXT: &str = include_str!("../../../tests/fixtures/context/unique.json");

fn inputs() -> Value {
    json!({
        "service":"fixture-web", "environment":"dev",
        "pack":{"id":"service-queries","version":1},
        "window":{"kind":"absolute","from":"2026-10-02T00:00:00Z","to":"2026-10-02T00:05:00Z"},
        "metrics":{"datasource":"metrics-1","parameters":{"job":"fixture"}},
        "logs":{"datasource":"logs-1","parameters":{"job":"fixture"}},
        "on_failure":"continue"
    })
}

fn datasource(uid: &str, kind: &str) -> Reply {
    Reply::json(json!({"uid":uid,"type":kind,"orgId":7}))
}

fn replies() -> Vec<Reply> {
    vec![
        Reply::org(),
        datasource("metrics-1", "prometheus"),
        Reply::query(),
        Reply::org(),
        datasource("logs-1", "loki"),
        Reply::query(),
    ]
}

struct EvidenceFixture {
    https: Fixture,
    source: PathBuf,
    state: PathBuf,
}

impl EvidenceFixture {
    fn new(replies: Vec<Reply>) -> Self {
        let https = Fixture::new(replies);
        let source = https.directory.join("context.json");
        let mut context: Value = serde_json::from_str(CONTEXT).unwrap();
        let record = &mut context["records"][0];
        record["service"] = json!("fixture-web");
        record["last_reviewed"] = json!(workbench_core::result::timestamp());
        record["bindings"] = json!({"target":"fixture","environment":"dev","organization":"7"});
        fs::write(&source, context.to_string()).unwrap();
        let state = https.directory.join("evidence");
        Self {
            https,
            source,
            state,
        }
    }

    fn command(&self) -> Command {
        let mut command = self.https.command();
        command
            .arg("--context-source")
            .arg(&self.source)
            .args(["--context-max-age-days", "30", "--state-dir"])
            .arg(&self.state);
        command
    }

    fn collect(&self, input: &Value) -> Command {
        self.collect_raw(&input.to_string(), "always")
    }

    fn collect_raw(&self, input: &str, record: &str) -> Command {
        let path = self.https.directory.join("collection-input.json");
        fs::write(&path, input).unwrap();
        let mut command = self.command();
        command
            .args([
                "--record", record, "evidence", "collect", "--target", "fixture", "--input",
            ])
            .arg(path);
        command
    }

    fn call(&self, input: Value) -> Command {
        let path = self.https.directory.join("collection-request.json");
        fs::write(
            &path,
            json!({
                "spec_version":"0.1", "request_id":"evidence-fixture",
                "operation":"evidence.collect", "operation_version":1,
                "target":{"kind":"connection","id":"fixture"},
                "inputs":input,
                "limits":{"timeout_ms":60000,"max_output_bytes":2097152},
                "record":"always"
            })
            .to_string(),
        )
        .unwrap();
        let mut command = self.command();
        command.args(["call", "--request"]).arg(path);
        command
    }

    fn manage(&self, arguments: &[&str]) -> Command {
        // A fresh process gets neither a provider nor context/Grafana configuration.
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .args(["--json", "--state-dir"])
            .arg(&self.state)
            .arg("evidence")
            .args(arguments);
        command
    }

    fn run(&self, command: &mut Command, exit: i32) -> Value {
        let (output, result) = self.https.run(command);
        assert_eq!(output.status.code(), Some(exit), "{result}");
        assert_eq!(result["assessment"], "not_assessed");
        assert!(result["finished_at"].as_str().unwrap() >= result["started_at"].as_str().unwrap());
        result
    }

    fn edit_context(&self, edit: impl FnOnce(&mut Value)) {
        let mut context: Value = serde_json::from_slice(&fs::read(&self.source).unwrap()).unwrap();
        edit(&mut context);
        fs::write(&self.source, context.to_string()).unwrap();
    }

    fn manifest(&self, result: &Value) -> (PathBuf, Value) {
        let reference = &result["data"]["bundle"];
        assert!(reference.is_object(), "Expected a durable bundle: {result}");
        let run = self
            .state
            .join("runs")
            .join(reference["id"].as_str().unwrap());
        let bytes = fs::read(run.join("manifest.json")).unwrap();
        assert_eq!(reference["digest"], sha256(&bytes));
        let manifest: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(manifest["id"], reference["id"]);
        assert_eq!(manifest["format_version"], 1);
        assert_eq!(manifest["state"], "sealed");
        assert!(manifest.get("digest").is_none());
        assert!(manifest["payload"].get("bundle").is_none());
        let mut payload = result["data"].clone();
        payload.as_object_mut().unwrap().remove("bundle");
        payload
            .as_object_mut()
            .unwrap()
            .remove("delivery_cancellation_signal");
        assert_eq!(manifest["payload"], payload);
        (run, manifest)
    }
}

fn sha256(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}

fn stored_receipt(run: &Path, reference: &Value) -> Value {
    assert_eq!(reference["media_type"], "application/json");
    assert_eq!(reference["classification"], "restricted");
    let digest = reference["digest"].as_str().unwrap();
    assert_eq!(
        reference["path"],
        format!("artifacts/{}.json", &digest[7..])
    );
    let bytes = fs::read(run.join(reference["path"].as_str().unwrap())).unwrap();
    assert_eq!(reference["bytes"].as_u64(), Some(bytes.len() as u64));
    assert_eq!(sha256(&bytes), digest);
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert!(!bytes[..bytes.len() - 1].contains(&b'\n'));
    let receipt: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(receipt["spec_version"], "0.1");
    assert_eq!(receipt["operation"], "query.run");
    assert_eq!(receipt["operation_version"], 1);
    assert_eq!(receipt["record_mode"], "never");
    assert_eq!(receipt["assessment"], "not_assessed");
    assert!(receipt["finished_at"].as_str().unwrap() >= receipt["started_at"].as_str().unwrap());
    receipt
}

fn store_files(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let metadata = entry.metadata().unwrap();
        if metadata.is_dir() {
            assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
            files.extend(store_files(&entry.path()));
        } else {
            assert!(metadata.is_file());
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
            files.push(entry.path());
        }
    }
    files.sort();
    files
}

fn wait_bounded(child: &mut Child, started: Instant) {
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(4) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("collection exceeded its cancellation/finalization allowance");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

// Independent calendar arithmetic, without using the production window parser.
fn epoch_millis(timestamp: &str) -> i64 {
    assert!(timestamp.ends_with('Z'));
    let year: i64 = timestamp[0..4].parse().unwrap();
    let month: usize = timestamp[5..7].parse().unwrap();
    let day: i64 = timestamp[8..10].parse().unwrap();
    let hour: i64 = timestamp[11..13].parse().unwrap();
    let minute: i64 = timestamp[14..16].parse().unwrap();
    let second: i64 = timestamp[17..19].parse().unwrap();
    let leap = |year: i64| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let lengths = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let days = (1970..year)
        .map(|year| if leap(year) { 366 } else { 365 })
        .sum::<i64>()
        + lengths[..month - 1].iter().sum::<i64>()
        + day
        - 1;
    let millis = timestamp[19..]
        .strip_prefix('.')
        .unwrap_or("")
        .bytes()
        .take_while(u8::is_ascii_digit)
        .chain(std::iter::repeat(b'0'))
        .take(3)
        .fold(0, |total, digit| total * 10 + i64::from(digit - b'0'));
    ((days * 24 + hour) * 3600 + minute * 60 + second) * 1000 + millis
}

#[test]
fn collection_runs_both_installed_queries_and_seals_exact_finalized_receipts() {
    let fixture = EvidenceFixture::new(replies());
    let described = fixture.run(
        fixture.https.command().args([
            "evidence",
            "packs",
            "describe",
            "service-queries",
            "--version",
            "1",
        ]),
        0,
    );
    let mut supplied = inputs();
    supplied["expected_pack_sha256"] = described["data"]["pack"]["sha256"].clone();
    let result = fixture.run(&mut fixture.collect(&supplied), 0);
    assert_eq!(result["operation"], "evidence.collect");
    assert_eq!(result["record_mode"], "always");
    assert_eq!(result["execution"]["status"], "succeeded");
    assert_eq!(
        result["data"]["capture"],
        json!({"state":"complete","expected_steps":2,"retained_receipts":2})
    );
    assert_eq!(result["data"]["selection"]["service"], "fixture-web");
    assert_eq!(result["data"]["connection"]["organization_id"], 7);
    assert_eq!(
        result["data"]["provenance"]["context"]["resolution"]["usable"],
        true
    );
    let (run, manifest) = fixture.manifest(&result);
    assert_eq!(manifest["artifacts"].as_object().unwrap().len(), 2);
    let steps = result["data"]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);
    let requests = fixture.https.requests();
    assert_eq!(requests.len(), 6);
    for (index, id, uid, kind, expression) in [
        (
            0,
            "metrics",
            "metrics-1",
            "prometheus",
            "up{job=\"fixture\"}",
        ),
        (1, "logs", "logs-1", "loki", "{job=\"fixture\"}"),
    ] {
        let step = &steps[index];
        assert_eq!(step["id"], id);
        assert_eq!(step["status"], "succeeded");
        assert_eq!(step["omission"], Value::Null);
        assert_eq!(step["artifact"], manifest["artifacts"][id]);
        let child = stored_receipt(&run, &step["artifact"]);
        assert_eq!(child["run_id"], step["run_id"]);
        assert_eq!(child["started_at"], step["started_at"]);
        assert_eq!(child["finished_at"], step["finished_at"]);
        assert_eq!(child["duration_ms"], step["duration_ms"]);
        assert_eq!(child["data"]["definition"], step["definition"]);
        assert_eq!(child["data"]["window"], result["data"]["window"]);
        assert_eq!(child["coverage"]["state"], "not_established");
        assert!(step.get("response").is_none());
        assert_eq!(requests[index * 3].path, "/grafana/api/org");
        assert_eq!(
            requests[index * 3 + 1].path,
            format!("/grafana/api/datasources/uid/{uid}?ds_type={kind}")
        );
        assert_eq!(requests[index * 3 + 2].method, "POST");
        assert_eq!(requests[index * 3 + 2].path, "/grafana/api/ds/query");
        let body: Value = serde_json::from_slice(&requests[index * 3 + 2].body).unwrap();
        assert_eq!(body["from"], "1790899200000");
        assert_eq!(body["to"], "1790899500000");
        assert_eq!(
            body["queries"][0]["datasource"],
            json!({"uid":uid,"type":kind})
        );
        assert_eq!(body["queries"][0]["expr"], expression);
    }
    for request in requests {
        assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
        assert_eq!(request.headers["x-grafana-org-id"], "7");
    }
    assert_eq!(
        fs::metadata(&fixture.state).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(!store_files(&fixture.state).is_empty());
}

#[test]
fn relative_window_is_resolved_once_from_the_collection_parent() {
    let mut supplied = inputs();
    supplied["window"] = json!({"kind":"relative","lookback_seconds":300});
    let fixture = EvidenceFixture::new(replies());
    let result = fixture.run(&mut fixture.collect_raw(&supplied.to_string(), "auto"), 0);
    assert_eq!(result["record_mode"], "auto");
    let to = epoch_millis(result["started_at"].as_str().unwrap());
    let from = to - 300_000;
    let window = &result["data"]["window"];
    assert_eq!(epoch_millis(window["to"].as_str().unwrap()), to);
    assert_eq!(epoch_millis(window["from"].as_str().unwrap()), from);
    let requests = fixture.https.requests();
    assert_eq!(requests.len(), 6);
    for index in [2, 5] {
        let body: Value = serde_json::from_slice(&requests[index].body).unwrap();
        assert_eq!(body["from"], from.to_string());
        assert_eq!(body["to"], to.to_string());
    }
}

#[test]
fn structured_collection_dispatches_the_same_pack_and_retains_the_same_observations() {
    let mut responses = replies();
    responses.extend(replies());
    let fixture = EvidenceFixture::new(responses);
    let direct = fixture.run(&mut fixture.collect(&inputs()), 0);
    let structured = fixture.run(&mut fixture.call(inputs()), 0);
    assert_eq!(structured["request_id"], "evidence-fixture");
    for key in [
        "selection",
        "connection",
        "pack",
        "window",
        "capture",
        "on_failure",
    ] {
        assert_eq!(direct["data"][key], structured["data"][key]);
    }
    assert_ne!(
        direct["data"]["bundle"]["id"],
        structured["data"]["bundle"]["id"]
    );
    let (direct_run, _) = fixture.manifest(&direct);
    let (structured_run, _) = fixture.manifest(&structured);
    for index in 0..2 {
        let one = stored_receipt(&direct_run, &direct["data"]["steps"][index]["artifact"]);
        let two = stored_receipt(
            &structured_run,
            &structured["data"]["steps"][index]["artifact"],
        );
        for key in ["definition", "parameters", "window", "request", "response"] {
            assert_eq!(one["data"][key], two["data"][key]);
        }
    }
    let requests = fixture.https.requests();
    assert_eq!(requests.len(), 12);
    for index in 0..6 {
        assert_eq!(requests[index].method, requests[index + 6].method);
        assert_eq!(requests[index].path, requests[index + 6].path);
        assert_eq!(requests[index].body, requests[index + 6].body);
    }
}

#[test]
fn installed_pack_discovery_requires_no_configuration_store_or_credentials() {
    let fixture = EvidenceFixture::new(vec![]);
    let missing = fixture.https.directory.join("missing-config.json");
    let command = |arguments: &[&str]| {
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .args(["--json", "--config"])
            .arg(&missing)
            .args(["evidence", "packs"])
            .args(arguments);
        command
    };
    let listed = fixture.run(&mut command(&["list"]), 0);
    let described = fixture.run(
        &mut command(&["describe", "service-queries", "--version", "1"]),
        0,
    );
    let packs = listed["data"]["packs"].as_array().unwrap();
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0], described["data"]["pack"]);
    assert_eq!(packs[0]["steps"].as_array().unwrap().len(), 2);
    assert_eq!(packs[0]["sha256"].as_str().unwrap().len(), 64);
    // Inspection itself never creates the nonexistent selected store or contacts Grafana.
    assert!(fixture.https.requests().is_empty());
    assert!(!fixture.state.exists());
    assert!(!missing.exists());
}

#[test]
fn malformed_or_unpinned_inputs_are_rejected_before_http_or_store_creation() {
    let fixture = EvidenceFixture::new(vec![]);
    for patch in [
        json!({"service":null}),
        json!({"environment":null}),
        json!({"team":null}),
        json!({"deployment":null}),
        json!({"pack":{"id":"service-queries","version":2}}),
        json!({"pack":{"id":"arbitrary","version":1}}),
        json!({"pack":{"id":"service-queries","version":1,"steps":[]}}),
        json!({"expected_pack_sha256":null}),
        json!({"expected_pack_sha256":"0".repeat(64)}),
        json!({"expected_pack_sha256":"A".repeat(64)}),
        json!({"expected_pack_sha256":"0".repeat(63)}),
        json!({"metrics":null}),
        json!({"metrics":{"datasource":"../metrics-1","parameters":{"job":"fixture"}}}),
        json!({"metrics":{"datasource":"metrics-1","parameters":{"job":"fixture"},"expr":"up"}}),
        json!({"logs":{"datasource":"logs-1","parameters":{"job":null}}}),
        json!({"logs":{"datasource":"logs-1","parameters":{"job":"fixture","unexpected":true}}}),
        json!({"logs":{"datasource":"logs-1","parameters":{"job":"$job"}}}),
        json!({"window":{"kind":"relative","lookback_seconds":0}}),
        json!({"window":{"kind":"relative","lookback_seconds":86401}}),
        json!({"window":{"kind":"absolute","from":"2026-10-02T00:00:00Z","to":"2026-10-02T00:00:00Z"}}),
        json!({"on_failure":null}),
        json!({"on_failure":"retry"}),
        json!({"state_dir":"/tmp/untrusted"}),
        json!({"steps":[]}),
    ] {
        let mut supplied = inputs();
        for (key, value) in patch.as_object().unwrap() {
            supplied[key] = value.clone();
        }
        let result = fixture.run(&mut fixture.call(supplied), 2);
        assert_eq!(
            result["effect_outcome"], "not_attempted",
            "patch {patch}: {result}"
        );
        assert!(!result["errors"].as_array().unwrap().is_empty());
        assert!(fixture.https.requests().is_empty(), "patch {patch}");
        assert!(!fixture.state.exists(), "patch {patch}");
    }
    let duplicate =
        inputs()
            .to_string()
            .replacen("\"service\":", "\"service\":\"duplicate\",\"service\":", 1);
    for raw in [duplicate.as_str(), "null", "[]"] {
        let result = fixture.run(&mut fixture.collect_raw(raw, "always"), 2);
        assert_eq!(result["effect_outcome"], "not_attempted");
        assert!(fixture.https.requests().is_empty());
        assert!(!fixture.state.exists());
    }
}

#[test]
fn collection_requires_durable_recording_before_any_effect() {
    let fixture = EvidenceFixture::new(vec![]);
    let result = fixture.run(&mut fixture.collect_raw(&inputs().to_string(), "never"), 2);
    assert!(has_error(&result, "recording_required"));
    assert_eq!(result["effect_outcome"], "not_attempted");
    assert!(fixture.https.requests().is_empty());
    assert!(!fixture.state.exists());
}

#[test]
fn context_selection_and_every_binding_are_required_before_store_or_http() {
    for change in [
        "target",
        "environment",
        "organization",
        "noncanonical_org",
        "absent_org",
        "stale",
        "inactive",
        "ambiguous",
        "missing",
    ] {
        let fixture = EvidenceFixture::new(vec![]);
        fixture.edit_context(|source| match change {
            "target" => source["records"][0]["bindings"]["target"] = json!("another-connection"),
            "environment" => source["records"][0]["bindings"]["environment"] = json!("prod"),
            "organization" => source["records"][0]["bindings"]["organization"] = json!("8"),
            "noncanonical_org" => source["records"][0]["bindings"]["organization"] = json!("07"),
            "absent_org" => {
                source["records"][0]["bindings"]
                    .as_object_mut()
                    .unwrap()
                    .remove("organization");
            }
            "stale" => source["records"][0]["last_reviewed"] = json!("2000-01-01T00:00:00Z"),
            "inactive" => source["records"][0]["state"] = json!("retired"),
            "ambiguous" => {
                let second = source["records"][0].clone();
                source["records"].as_array_mut().unwrap().push(second);
            }
            "missing" => source["records"][0]["service"] = json!("another-service"),
            _ => unreachable!(),
        });
        let result = fixture.run(&mut fixture.collect(&inputs()), 2);
        assert_eq!(
            result["effect_outcome"], "not_attempted",
            "{change}: {result}"
        );
        assert!(!result["errors"].as_array().unwrap().is_empty(), "{change}");
        assert!(fixture.https.requests().is_empty(), "{change}");
        assert!(!fixture.state.exists(), "{change}");
    }
}

#[test]
fn continue_and_stop_keep_failed_receipts_and_make_later_omissions_explicit() {
    for policy in ["continue", "stop"] {
        let mut failed = Reply::json(json!({"message":"synthetic query refusal"}));
        failed.status = 503;
        let fixture = EvidenceFixture::new(vec![
            Reply::org(),
            datasource("metrics-1", "prometheus"),
            failed,
            Reply::org(),
            datasource("logs-1", "loki"),
            Reply::query(),
        ]);
        let mut supplied = inputs();
        supplied["on_failure"] = json!(policy);
        let result = fixture.run(&mut fixture.collect(&supplied), 1);
        assert_ne!(result["execution"]["status"], "succeeded");
        assert_eq!(result["data"]["capture"]["state"], "partial");
        let (run, manifest) = fixture.manifest(&result);
        let first = &result["data"]["steps"][0];
        assert_eq!(first["status"], "failed");
        let stored = stored_receipt(&run, &first["artifact"]);
        assert_eq!(stored["execution"]["status"], "failed");
        let second = &result["data"]["steps"][1];
        if policy == "continue" {
            assert_eq!(second["status"], "succeeded");
            assert_eq!(result["data"]["capture"]["retained_receipts"], 2);
            assert_eq!(manifest["artifacts"].as_object().unwrap().len(), 2);
            assert_eq!(fixture.https.requests().len(), 6);
        } else {
            assert_eq!(second["status"], "omitted");
            assert_eq!(second["omission"], "after_failure");
            assert_eq!(second["artifact"], Value::Null);
            assert_eq!(second["run_id"], Value::Null);
            assert_eq!(result["data"]["capture"]["retained_receipts"], 1);
            assert_eq!(manifest["artifacts"].as_object().unwrap().len(), 1);
            assert_eq!(fixture.https.requests().len(), 3);
        }
    }
}

#[test]
fn read_after_restart_and_protection_work_without_provider_or_network() {
    let fixture = EvidenceFixture::new(replies());
    let collected = fixture.run(&mut fixture.collect(&inputs()), 0);
    let reference = &collected["data"]["bundle"];
    let id = reference["id"].as_str().unwrap();
    let digest = reference["digest"].as_str().unwrap();
    let (_, manifest) = fixture.manifest(&collected);
    let listed = fixture.run(&mut fixture.manage(&["list"]), 0);
    assert_eq!(listed["record_mode"], "never");
    assert_eq!(listed["data"]["runs"].as_array().unwrap().len(), 1);
    assert_eq!(listed["data"]["runs"][0]["id"], id);
    assert_eq!(listed["data"]["runs"][0]["state"], "sealed");
    let shown = fixture.run(
        &mut fixture.manage(&["show", id, "--expected-manifest-sha256", digest]),
        0,
    );
    assert_eq!(shown["data"]["bundle"], *reference);
    assert_eq!(shown["data"]["manifest"], manifest);
    let wrong = format!("sha256:{}", "0".repeat(64));
    let refused = fixture.run(
        &mut fixture.manage(&["delete", id, "--expected-manifest-sha256", &wrong]),
        1,
    );
    assert_eq!(refused["effect_outcome"], "not_attempted");
    assert!(fixture.state.join("runs").join(id).exists());
    let protected = fixture.run(&mut fixture.manage(&["protect", id]), 0);
    assert_eq!(protected["data"]["protected"], true);
    let shown = fixture.run(&mut fixture.manage(&["show", id]), 0);
    assert_eq!(shown["data"]["protected"], true);
    let refused = fixture.run(&mut fixture.manage(&["delete", id]), 1);
    assert!(has_error(&refused, "store_protected"));
    fixture.run(&mut fixture.manage(&["prune"]), 0);
    assert!(fixture.state.join("runs").join(id).exists());
    let unprotected = fixture.run(&mut fixture.manage(&["unprotect", id]), 0);
    assert_eq!(unprotected["data"]["protected"], false);
    fixture.run(&mut fixture.manage(&["delete", id]), 0);
    assert!(!fixture.state.join("runs").join(id).exists());
    let listed = fixture.run(&mut fixture.manage(&["list"]), 0);
    assert_eq!(listed["data"]["runs"], json!([]));
    assert_eq!(fixture.https.requests().len(), 6);
}

#[test]
fn interrupted_delete_retry_preserves_and_checks_the_original_manifest_digest() {
    for removed_directory in [false, true] {
        let fixture = EvidenceFixture::new(replies());
        let collected = fixture.run(&mut fixture.collect(&inputs()), 0);
        let reference = &collected["data"]["bundle"];
        let id = reference["id"].as_str().unwrap();
        let digest = reference["digest"].as_str().unwrap();
        let (run, _) = fixture.manifest(&collected);
        let journal = fixture.state.join(format!(".delete-{id}.json"));
        // Model the two persisted states on either side of the final run-directory unlink.
        // The store's fault-injection tests exercise reaching both states during deletion.
        fs::rename(run.join("manifest.json"), &journal).unwrap();
        fs::remove_file(run.join("intent.json")).unwrap();
        fs::remove_file(run.join("checkpoint.json")).unwrap();
        fs::remove_dir_all(run.join("artifacts")).unwrap();
        if removed_directory {
            fs::remove_dir(&run).unwrap();
        }
        let original = fs::read(&journal).unwrap();
        let wrong = format!("sha256:{}", "0".repeat(64));
        let refused = fixture.run(
            &mut fixture.manage(&["delete", id, "--expected-manifest-sha256", &wrong]),
            1,
        );
        assert!(has_error(&refused, "manifest_digest_mismatch"));
        assert_eq!(refused["effect_outcome"], "not_attempted");
        assert_eq!(fs::read(&journal).unwrap(), original);
        assert_eq!(run.exists(), !removed_directory);
        let deleted = fixture.run(
            &mut fixture.manage(&["delete", id, "--expected-manifest-sha256", digest]),
            0,
        );
        assert_eq!(deleted["data"]["bundle"], *reference);
        assert!(!journal.exists());
        assert!(!run.exists());
        assert_eq!(fixture.https.requests().len(), 6);
    }
}

#[test]
fn bulk_management_refuses_unknown_payloads_and_inconsistent_receipt_summaries() {
    for corruption in ["future-version", "summary-mismatch"] {
        for interrupted in [false, true] {
            let fixture = EvidenceFixture::new(replies());
            let collected = fixture.run(&mut fixture.collect(&inputs()), 0);
            let (run, _) = fixture.manifest(&collected);
            let id = collected["run_id"].as_str().unwrap();
            for name in ["intent.json", "checkpoint.json", "manifest.json"] {
                let path = run.join(name);
                let mut metadata: Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                if metadata.get("created_at_unix_ms").is_some() {
                    metadata["created_at_unix_ms"] = json!(1);
                }
                if metadata.get("finalized_at_unix_ms").is_some() {
                    metadata["finalized_at_unix_ms"] = json!(2);
                }
                if corruption == "future-version" {
                    metadata["payload"]["format_version"] = json!(2);
                } else if name != "intent.json" {
                    let duration = metadata["payload"]["steps"][0]["duration_ms"]
                        .as_u64()
                        .unwrap();
                    metadata["payload"]["steps"][0]["duration_ms"] = json!(duration + 1);
                }
                fs::write(path, format!("{metadata}\n")).unwrap();
            }
            if interrupted {
                fs::remove_file(run.join("manifest.json")).unwrap();
            }
            let snapshot = || {
                store_files(&fixture.state)
                    .into_iter()
                    .map(|path| {
                        let bytes = fs::read(&path).unwrap();
                        (path, bytes)
                    })
                    .collect::<std::collections::BTreeMap<_, _>>()
            };
            let before = snapshot();
            let listed = fixture.run(&mut fixture.manage(&["list"]), 0);
            assert_eq!(listed["data"]["runs"][0]["state"], "corrupt", "{listed}");
            assert!(listed["data"]["runs"][0]["error"].is_string());
            if !interrupted {
                fixture.run(&mut fixture.manage(&["show", id]), 1);
                fixture.run(&mut fixture.manage(&["delete", id]), 1);
            }
            let refused = fixture.run(
                &mut fixture.manage(&[if interrupted { "recover" } else { "prune" }]),
                1,
            );
            assert_eq!(refused["effect_outcome"], "not_attempted", "{refused}");
            assert_eq!(
                snapshot(),
                before,
                "{corruption}, interrupted={interrupted}"
            );
            assert_eq!(fixture.https.requests().len(), 6);
        }
    }
}

#[test]
fn changed_stored_receipt_bytes_are_detected_without_replaying_queries() {
    let fixture = EvidenceFixture::new(replies());
    let result = fixture.run(&mut fixture.collect(&inputs()), 0);
    let (run, manifest) = fixture.manifest(&result);
    let artifact = run.join(manifest["artifacts"]["metrics"]["path"].as_str().unwrap());
    let mut bytes = fs::read(&artifact).unwrap();
    bytes[0] = b'[';
    fs::write(&artifact, bytes).unwrap();
    let id = result["data"]["bundle"]["id"].as_str().unwrap();
    let shown = fixture.run(&mut fixture.manage(&["show", id]), 1);
    assert_eq!(shown["effect_outcome"], "not_attempted");
    assert!(!shown["errors"].as_array().unwrap().is_empty());
    let listed = fixture.run(&mut fixture.manage(&["list"]), 0);
    assert_eq!(listed["data"]["runs"][0]["state"], "corrupt");
    assert_eq!(fixture.https.requests().len(), 6);
}

#[test]
fn unsafe_or_underfunded_stores_refuse_collection_before_http() {
    for layout in ["symlink", "public_mode", "regular_file", "quota"] {
        let fixture = EvidenceFixture::new(vec![]);
        match layout {
            "symlink" => {
                let outside = fixture.https.directory.join("outside");
                fs::create_dir(&outside).unwrap();
                fs::set_permissions(&outside, fs::Permissions::from_mode(0o700)).unwrap();
                symlink(&outside, &fixture.state).unwrap();
            }
            "public_mode" => {
                fs::create_dir(&fixture.state).unwrap();
                fs::set_permissions(&fixture.state, fs::Permissions::from_mode(0o755)).unwrap();
            }
            "regular_file" => fs::write(&fixture.state, b"operator-owned marker").unwrap(),
            "quota" => {}
            _ => unreachable!(),
        }
        let mut command = fixture.collect(&inputs());
        if layout == "quota" {
            command.args(["--store-max-bytes", "1024"]);
        }
        let (output, result) = fixture.https.run(&mut command);
        assert!(!output.status.success(), "{layout}: {result}");
        assert!(!result["errors"].as_array().unwrap().is_empty(), "{layout}");
        assert_ne!(result["data"]["capture"]["state"], "complete");
        assert!(fixture.https.requests().is_empty(), "{layout}");
        if layout == "regular_file" {
            assert_eq!(fs::read(&fixture.state).unwrap(), b"operator-owned marker");
        }
    }
}

#[test]
fn private_credential_canaries_do_not_reappear_in_projection_or_persisted_files() {
    let sensitive = Reply::json(
        json!({"results":{"A":{"frames":[{"line":format!("before {TOKEN} after")} ]}}}),
    );
    let fixture = EvidenceFixture::new(vec![
        Reply::org(),
        datasource("metrics-1", "prometheus"),
        sensitive,
        Reply::org(),
        datasource("logs-1", "loki"),
        Reply::query(),
    ]);
    fixture.edit_context(|source| {
        source["source"]["id"] = json!(format!("source-{TOKEN}"));
        source["source"]["revision"] = json!(TOKEN);
        source["records"][0]["team"] = json!(TOKEN);
        source["records"][0]["deployment"] = json!(TOKEN);
    });
    let mut supplied = inputs();
    supplied["team"] = json!(TOKEN);
    supplied["deployment"] = json!(TOKEN);
    supplied["metrics"]["parameters"]["job"] = json!(TOKEN);
    let result = fixture.run(&mut fixture.collect(&supplied), 0);
    assert!(!result.to_string().contains(TOKEN));
    assert_eq!(
        result["data"]["selection"]["team"],
        json!({"redacted":true})
    );
    assert_eq!(
        result["data"]["provenance"]["context"]["source"]["revision"],
        json!({"redacted":true})
    );
    assert_eq!(result["data"]["redaction"]["applied"], true);
    assert!(
        result["data"]["redaction"]["replacements"]
            .as_u64()
            .unwrap()
            > 0
    );
    for file in store_files(&fixture.state) {
        let bytes = fs::read(&file).unwrap();
        assert!(
            !bytes
                .windows(TOKEN.len())
                .any(|window| window == TOKEN.as_bytes()),
            "{file:?}"
        );
    }
    let (run, manifest) = fixture.manifest(&result);
    let metrics = stored_receipt(&run, &manifest["artifacts"]["metrics"]);
    assert_eq!(metrics["data"]["redaction"]["applied"], true);
    assert_eq!(fixture.https.requests().len(), 6);
}

#[test]
fn parent_deadline_limits_the_second_query_and_retains_both_real_outcomes() {
    let mut responses = replies();
    for response in &mut responses[..3] {
        response.header_delay = Duration::from_millis(90);
    }
    responses[4].header_delay = Duration::from_secs(3);
    let fixture = EvidenceFixture::new(responses);
    let started = Instant::now();
    let result = fixture.run(fixture.collect(&inputs()).args(["--timeout", "900ms"]), 1);
    assert_eq!(result["execution"]["status"], "timed_out");
    assert_eq!(result["effective_limits"]["timeout_ms"], 900);
    assert_eq!(result["data"]["capture"]["state"], "partial");
    assert_eq!(result["data"]["steps"][0]["status"], "succeeded");
    assert_eq!(result["data"]["steps"][1]["status"], "timed_out");
    let (run, manifest) = fixture.manifest(&result);
    let first = stored_receipt(&run, &manifest["artifacts"]["metrics"]);
    let second = stored_receipt(&run, &manifest["artifacts"]["logs"]);
    let first_duration = first["duration_ms"].as_u64().unwrap();
    assert!(first_duration >= 200);
    assert!(second["effective_limits"]["timeout_ms"].as_u64().unwrap() <= 900 - first_duration);
    assert_eq!(fixture.https.requests().len(), 5);
    assert!(started.elapsed() < Duration::from_secs(4));
    thread::sleep(Duration::from_millis(40));
    assert_eq!(fixture.https.requests().len(), 5);
}

#[test]
fn signals_cancel_active_queries_and_seal_only_already_observed_receipts() {
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let mut responses = replies();
        responses[2].body_delay = Duration::from_secs(5);
        let fixture = EvidenceFixture::new(responses);
        let mut child = fixture
            .collect(&inputs())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        fixture.https.wait_for_requests(3);
        let started = Instant::now();
        // SAFETY: the PID belongs to this still-owned CLI child in the private test namespace.
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        wait_bounded(&mut child, started);
        let output = child.wait_with_output().unwrap();
        let result = receipt(&output);
        assert_eq!(output.status.code(), Some(128 + signal), "{result}");
        assert_eq!(result["execution"]["status"], "cancelled");
        assert_eq!(result["data"]["cancellation_signal"], signal);
        assert_eq!(result["data"]["capture"]["state"], "partial");
        assert_eq!(result["data"]["steps"][0]["status"], "cancelled");
        assert_eq!(result["data"]["steps"][1]["status"], "omitted");
        assert_eq!(result["data"]["steps"][1]["omission"], "cancelled");
        let (run, manifest) = fixture.manifest(&result);
        let child = stored_receipt(&run, &manifest["artifacts"]["metrics"]);
        assert_eq!(child["execution"]["status"], "cancelled");
        assert_eq!(child["data"]["cancellation_signal"], signal);
        assert_eq!(manifest["artifacts"].as_object().unwrap().len(), 1);
        assert_eq!(fixture.https.requests().len(), 3);
        thread::sleep(Duration::from_millis(40));
        assert_eq!(fixture.https.requests().len(), 3);
    }
}

#[test]
fn closed_result_consumer_stops_active_query_and_later_dispatch() {
    let mut responses = replies();
    responses[2].body_delay = Duration::from_secs(5);
    let fixture = EvidenceFixture::new(responses);
    let mut child = fixture
        .collect(&inputs())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    fixture.https.wait_for_requests(3);
    let started = Instant::now();
    drop(child.stdout.take());
    wait_bounded(&mut child, started);
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(fixture.https.requests().len(), 3);
    let listed = fixture.run(&mut fixture.manage(&["list"]), 0);
    assert_eq!(listed["data"]["runs"].as_array().unwrap().len(), 1);
    let id = listed["data"]["runs"][0]["id"].as_str().unwrap();
    let shown = fixture.run(&mut fixture.manage(&["show", id]), 0);
    assert_ne!(
        shown["data"]["manifest"]["payload"]["capture"]["state"],
        "complete"
    );
    thread::sleep(Duration::from_millis(40));
    assert_eq!(fixture.https.requests().len(), 3);
}

#[test]
fn interrupted_process_is_recovered_without_replaying_or_claiming_complete_capture() {
    let mut responses = replies();
    responses[3].body_delay = Duration::from_secs(5);
    let fixture = EvidenceFixture::new(responses);
    let mut child = fixture
        .collect(&inputs())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Starting the second query establishes that the first finalized receipt was checkpointed.
    fixture.https.wait_for_requests(4);
    child.kill().unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let listed = fixture.run(&mut fixture.manage(&["list"]), 0);
    let runs = listed["data"]["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["state"], "incomplete");
    let id = runs[0]["id"].as_str().unwrap();
    let run = fixture.state.join("runs").join(id);
    assert!(!run.join("manifest.json").exists());
    let checkpoint: Value =
        serde_json::from_slice(&fs::read(run.join("checkpoint.json")).unwrap()).unwrap();
    let retained = checkpoint["artifacts"]["metrics"].clone();
    let before = stored_receipt(&run, &retained);
    fixture.run(&mut fixture.manage(&["show", id]), 1);
    assert!(
        !run.join("manifest.json").exists(),
        "read-only show must not recover the run"
    );
    fixture.run(&mut fixture.manage(&["recover"]), 0);
    let shown = fixture.run(&mut fixture.manage(&["show", id]), 0);
    assert_eq!(shown["data"]["manifest"]["state"], "interrupted");
    assert_eq!(shown["data"]["effective_capture"]["state"], "partial");
    assert_eq!(shown["data"]["manifest"]["artifacts"]["metrics"], retained);
    assert_eq!(stored_receipt(&run, &retained), before);
    fixture.run(&mut fixture.manage(&["prune"]), 0);
    assert!(run.exists());
    assert_eq!(fixture.https.requests().len(), 4);
    // Recovery is idempotent and never invents the unanswered logs observation.
    let manifest_bytes = fs::read(run.join("manifest.json")).unwrap();
    fixture.run(&mut fixture.manage(&["recover"]), 0);
    assert_eq!(fs::read(run.join("manifest.json")).unwrap(), manifest_bytes);
    assert_eq!(fixture.https.requests().len(), 4);
}

#[test]
fn newer_store_format_is_preserved_by_read_collection_and_management() {
    let fixture = EvidenceFixture::new(replies());
    let result = fixture.run(&mut fixture.collect(&inputs()), 0);
    let id = result["data"]["bundle"]["id"].as_str().unwrap();
    fs::write(
        fixture.state.join("format.json"),
        b"{\"format_version\":2}\n",
    )
    .unwrap();
    let before: Vec<_> = store_files(&fixture.state)
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    for arguments in [
        vec!["list"],
        vec!["show", id],
        vec!["delete", id],
        vec!["prune"],
        vec!["recover"],
    ] {
        let result = fixture.run(&mut fixture.manage(&arguments), 1);
        assert_eq!(result["effect_outcome"], "not_attempted");
        assert!(!result["errors"].as_array().unwrap().is_empty());
    }
    fixture.run(&mut fixture.collect(&inputs()), 1);
    for (path, bytes) in before {
        assert_eq!(fs::read(&path).unwrap(), bytes, "{path:?}");
    }
    assert_eq!(fixture.https.requests().len(), 6);
}

#[test]
fn concurrent_collection_cannot_bypass_the_store_lock_or_dispatch_queries() {
    let mut responses = replies();
    responses[2].body_delay = Duration::from_secs(5);
    let fixture = EvidenceFixture::new(responses);
    let mut child = fixture
        .collect(&inputs())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    fixture.https.wait_for_requests(3);
    let started = Instant::now();
    let contender = fixture.run(&mut fixture.collect(&inputs()), 1);
    assert!(has_error(&contender, "store_busy"));
    assert_eq!(contender["effect_outcome"], "not_attempted");
    assert_eq!(contender["data"]["bundle"], Value::Null);
    assert_eq!(fixture.https.requests().len(), 3);
    assert!(started.elapsed() < Duration::from_secs(2));
    // SAFETY: the first child is still owned and blocked in the private fixture.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
    wait_bounded(&mut child, Instant::now());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(143), "{}", receipt(&output));
    assert_eq!(fixture.https.requests().len(), 3);
}

fn response_at_receipt_depth_boundary(depth: usize, leaf: Value) -> Value {
    // The response's root, results, A, frames array and frame object account for
    // five container levels. Additional arrays give an exact HTTP input depth.
    let mut nested = leaf;
    for _ in 5..depth {
        nested = json!([nested]);
    }
    json!({"results":{"A":{"frames":[{"nested":nested}]}}})
}

fn direct_query_at_receipt_depth_boundary(fixture: &EvidenceFixture) -> Command {
    let parameters = fixture.https.directory.join("depth-parameters.json");
    let window = fixture.https.directory.join("depth-window.json");
    fs::write(&parameters, json!({"job":"fixture"}).to_string()).unwrap();
    fs::write(&window, inputs()["window"].to_string()).unwrap();
    let mut command = fixture.https.command();
    command
        .args([
            "queries",
            "run",
            "prometheus.up-by-job",
            "--version",
            "1",
            "--target",
            "fixture",
            "--datasource",
            "metrics-1",
            "--parameters",
        ])
        .arg(parameters)
        .arg("--window")
        .arg(window);
    command
}

#[test]
fn accepted_response_depth_survives_receipt_envelope_redaction_and_restart() {
    for depth in [64, 65] {
        let response = response_at_receipt_depth_boundary(depth, json!(741829));
        let mut fixture = EvidenceFixture::new(vec![
            Reply::org(),
            datasource("metrics-1", "prometheus"),
            Reply::json(response.clone()),
            Reply::org(),
            datasource("metrics-1", "prometheus"),
            Reply::json(response),
            Reply::org(),
            datasource("logs-1", "loki"),
            Reply::query(),
        ]);
        // A credential-valued number becomes a marker object, adding another
        // container after HTTP admission, before the result/data envelope.
        fixture.https.provider["auth"]["token"] = json!("741829");
        let exit = if depth > 64 { 1 } else { 0 };
        let direct = fixture.run(&mut direct_query_at_receipt_depth_boundary(&fixture), exit);
        let expected_response = if depth == 64 {
            let expected = response_at_receipt_depth_boundary(depth, json!({"redacted":true}));
            assert_eq!(direct["execution"]["status"], "succeeded");
            assert_eq!(direct["data"]["response"], expected);
            assert_eq!(direct["data"]["redaction"]["applied"], true);
            expected
        } else {
            assert_eq!(direct["execution"]["status"], "failed");
            assert!(has_error(&direct, "invalid_response"));
            assert_eq!(direct["data"]["response"], Value::Null);
            Value::Null
        };
        assert_eq!(fixture.https.requests().len(), 3);
        let collected = fixture.run(&mut fixture.collect(&inputs()), exit);
        assert_eq!(
            collected["data"]["capture"]["state"],
            if depth == 64 { "complete" } else { "partial" }
        );
        assert_eq!(collected["data"]["capture"]["retained_receipts"], 2);
        let (run, manifest) = fixture.manifest(&collected);
        let retained = stored_receipt(&run, &manifest["artifacts"]["metrics"]);
        assert_eq!(retained["data"]["response"], expected_response);
        assert_eq!(
            retained["execution"]["status"],
            direct["execution"]["status"]
        );
        if depth == 65 {
            assert!(has_error(&retained, "invalid_response"));
            assert_eq!(collected["data"]["steps"][0]["status"], "failed");
            assert_eq!(collected["data"]["steps"][1]["status"], "succeeded");
        }
        let id = collected["data"]["bundle"]["id"].as_str().unwrap();
        // A fresh process revalidates every stored receipt without credentials.
        let shown = fixture.run(&mut fixture.manage(&["show", id]), 0);
        assert_eq!(shown["data"]["manifest"], manifest);
        assert_eq!(
            stored_receipt(&run, &manifest["artifacts"]["metrics"]),
            retained
        );
        assert_eq!(fixture.https.requests().len(), 9);
    }
}
