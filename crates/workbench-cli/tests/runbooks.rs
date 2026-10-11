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
        // Fresh processes carry no provider, context, or Grafana configuration.
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .args(["--json", "--state-dir"])
            .arg(self.https.directory.join("state"))
            .arg("--runbook-state-dir")
            .arg(self.https.directory.join("journal"));
        command
    }

    fn run(&self, command: &mut Command, exit: i32) -> Value {
        let (output, result) = self.https.run(command);
        assert_eq!(output.status.code(), Some(exit), "{result}");
        assert_eq!(result["assessment"], "not_assessed");
        result
    }

    fn snapshot(&self, directory: &str) -> BTreeMap<String, Vec<u8>> {
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
        let root = self.https.directory.join(directory);
        let mut output = BTreeMap::new();
        if root.exists() {
            visit(&root, &root, &mut output);
        }
        output
    }
}

impl Mission {
    fn pair(second: Value) -> (Self, Value) {
        Self::pair_window(second, input())
    }

    fn pair_window(second: Value, after_input: Value) -> (Self, Value) {
        let mut replies =
            collection_replies(vec![metric("a", 0, json!([1, 1]))], logs(0, &["same"]));
        replies.extend(collection_replies(vec![second], logs(0, &["same"])));
        let mission = Self::new(replies);
        let before = mission.collect(&input(), 0);
        let after = mission.collect(&after_input, 0);
        let inputs = json!({"before":{"id":before["id"],"expected_manifest_sha256":before["digest"]},
            "after":{"id":after["id"],"expected_manifest_sha256":after["digest"]},
            "normalization":"exact-window-v1","on_failure":"continue"});
        (mission, inputs)
    }

    fn input_file(&self, value: &Value) -> std::path::PathBuf {
        let path = self.https.directory.join("runbook-input.json");
        fs::write(&path, value.to_string()).unwrap();
        path
    }

    fn start(&self, value: &Value) -> Command {
        let mut command = self.local();
        command
            .args([
                "--record",
                "always",
                "runbook",
                "run",
                "evidence-review",
                "--input",
            ])
            .arg(self.input_file(value));
        command
    }

    fn resume(&self, receipt: &Value, value: &Value, ack: Option<&str>) -> Command {
        let mut command = self.local();
        command
            .args(["--record", "always", "runbook", "resume"])
            .arg(receipt["data"]["journal"]["id"].as_str().unwrap())
            .arg("--expected-revision")
            .arg(
                receipt["data"]["journal"]["revision"]
                    .as_u64()
                    .unwrap()
                    .to_string(),
            )
            .arg("--input")
            .arg(self.input_file(value));
        if let Some(ack) = ack {
            command.args(["--acknowledge", ack]);
        }
        command
    }

    fn status(&self, receipt: &Value) -> Value {
        self.run(
            self.local()
                .args(["runbook", "status"])
                .arg(receipt["data"]["journal"]["id"].as_str().unwrap()),
            0,
        )
    }
}

fn error(receipt: &Value, code: &str) {
    assert!(
        receipt["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["code"] == code),
        "{receipt}"
    );
}

fn observations(receipt: &Value) -> Vec<Value> {
    receipt["data"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|step| !step["receipt_summary"].is_null())
        .cloned()
        .collect()
}

#[test]
fn runbook_pauses_and_resumes_across_processes_without_repeating_observations() {
    let (mission, inputs) = Mission::pair(metric("a", 0, json!([1, 0])));
    let evidence = mission.snapshot("state");
    let waiting = mission.run(&mut mission.start(&inputs), 1);
    assert_eq!(waiting["data"]["workflow_state"], "waiting");
    assert_eq!(waiting["data"]["pending_checkpoint"], "review-complete");
    assert_eq!(waiting["data"]["observation_state"], "complete");
    let observed = observations(&waiting);
    assert_eq!(observed.len(), 3, "{waiting}");
    let disk = mission.snapshot("journal");
    let status = mission.status(&waiting);
    assert_eq!(status["data"], waiting["data"]);
    assert_eq!(mission.snapshot("journal"), disk);
    let complete = mission.run(
        &mut mission.resume(&waiting, &inputs, Some("review-complete")),
        0,
    );
    assert_eq!(complete["data"]["workflow_state"], "completed");
    assert!(complete["data"]["pending_checkpoint"].is_null());
    assert!(
        complete["data"]["journal"]["revision"].as_u64().unwrap()
            > waiting["data"]["journal"]["revision"].as_u64().unwrap()
    );
    assert_eq!(observations(&complete), observed);
    assert_eq!(mission.status(&complete)["data"], complete["data"]);
    assert_eq!(mission.snapshot("state"), evidence);
    assert_eq!(mission.https.requests().len(), 12);
}

#[test]
fn runbook_refuses_wrong_ack_stale_revision_changed_input_and_definition_without_writes() {
    let (mission, inputs) = Mission::pair(metric("a", 0, json!([1, 0])));
    let waiting = mission.run(&mut mission.start(&inputs), 1);
    let disk = mission.snapshot("journal");
    let wrong = mission.run(
        &mut mission.resume(&waiting, &inputs, Some("review-partial")),
        2,
    );
    error(&wrong, "runbook_checkpoint_mismatch");
    assert_eq!(mission.snapshot("journal"), disk);
    let mut stale = waiting.clone();
    stale["data"]["journal"]["revision"] = json!(1);
    let refused = mission.run(
        &mut mission.resume(&stale, &inputs, Some("review-complete")),
        2,
    );
    error(&refused, "runbook_revision_mismatch");
    let mut changed = inputs.clone();
    changed["normalization"] = json!("window-offset-v1");
    let refused = mission.run(
        &mut mission.resume(&waiting, &changed, Some("review-complete")),
        2,
    );
    error(&refused, "runbook_input_mismatch");
    let refused = mission.run(
        mission
            .start(&inputs)
            .args(["--expected-definition-sha256", &"0".repeat(64)]),
        2,
    );
    error(&refused, "runbook_definition_mismatch");
    assert_eq!(mission.snapshot("journal"), disk);
    assert_eq!(mission.https.requests().len(), 12);
}

#[test]
fn runbook_partial_and_incompatible_observations_select_distinct_human_checkpoints() {
    for partial in [true, false] {
        let (mission, inputs, checkpoint, state) = if partial {
            let (m, i) = Mission::pair(metric("a", 0, json!([1, null])));
            (m, i, "review-partial", "partial")
        } else {
            let mut shifted = input();
            shifted["window"]["to"] = json!("2026-10-02T00:06:00Z");
            let (m, i) = Mission::pair_window(metric("a", 0, json!([1, 1])), shifted);
            (m, i, "review-incompatible", "incompatible")
        };
        let waiting = mission.run(&mut mission.start(&inputs), 1);
        assert_eq!(waiting["data"]["observation_state"], state, "{waiting}");
        assert_eq!(waiting["data"]["pending_checkpoint"], checkpoint);
        let complete = mission.run(&mut mission.resume(&waiting, &inputs, Some(checkpoint)), 1);
        assert_eq!(complete["data"]["workflow_state"], "completed");
        assert_eq!(complete["data"]["observation_state"], state);
        assert_eq!(observations(&waiting), observations(&complete));
        assert_eq!(mission.https.requests().len(), 12);
    }
}

#[test]
fn explicit_fresh_attempt_gets_new_id_and_preserves_old_run() {
    let (mission, inputs) = Mission::pair(metric("a", 0, json!([1, 1])));
    let first = mission.run(&mut mission.start(&inputs), 1);
    let old = mission.snapshot("journal");
    let second = mission.run(
        mission
            .start(&inputs)
            .arg("--fresh-from")
            .arg(first["data"]["journal"]["id"].as_str().unwrap())
            .arg("--fresh-from-revision")
            .arg(
                first["data"]["journal"]["revision"]
                    .as_u64()
                    .unwrap()
                    .to_string(),
            ),
        1,
    );
    assert_ne!(
        first["data"]["journal"]["id"],
        second["data"]["journal"]["id"]
    );
    assert_ne!(observations(&first), observations(&second));
    let new = mission.snapshot("journal");
    for (path, bytes) in old {
        assert_eq!(
            new.get(&path),
            Some(&bytes),
            "changed old journal file {path}"
        );
    }
    assert_eq!(mission.status(&first)["data"], first["data"]);
    assert_eq!(mission.https.requests().len(), 12);
}

#[test]
fn invalid_admission_and_readonly_discovery_never_initialize_journal() {
    let mission = Mission::new(vec![]);
    mission.run(mission.local().args(["runbook", "list"]), 0);
    mission.run(
        mission
            .local()
            .args(["runbook", "show", "evidence-review", "--version", "1"]),
        0,
    );
    let valid = json!({"before":{"id":"run-unknown"},"after":{"id":"run-unknown"},"normalization":"exact-window-v1","on_failure":"continue"});
    for changed in [
        {
            let mut v = valid.clone();
            v["normalization"] = json!("invented");
            v
        },
        {
            let mut v = valid.clone();
            v["before"]["id"] = json!("../../etc/passwd");
            v
        },
        {
            let mut v = valid.clone();
            v["surprise"] = json!(true);
            v
        },
    ] {
        mission.run(&mut mission.start(&changed), 2);
        assert!(!mission.https.directory.join("journal").exists());
    }
    mission.run(mission.start(&valid).args(["--version", "999"]), 2);
    mission.run(
        mission
            .local()
            .args(["runbook", "run", "evidence-review", "--input"])
            .arg(mission.input_file(&valid)),
        2,
    );
    assert!(!mission.https.directory.join("journal").exists());
    assert!(mission.https.requests().is_empty());
}

#[test]
fn changed_source_after_waiting_refuses_resume_without_mutating_checkpoint() {
    let (mission, inputs) = Mission::pair(metric("a", 0, json!([1, 1])));
    let waiting = mission.run(&mut mission.start(&inputs), 1);
    let journal = mission.snapshot("journal");
    let manifest = mission
        .https
        .directory
        .join("state/runs")
        .join(inputs["before"]["id"].as_str().unwrap())
        .join("manifest.json");
    fs::remove_file(manifest).unwrap();
    let refused = mission.run(
        &mut mission.resume(&waiting, &inputs, Some("review-complete")),
        2,
    );
    error(&refused, "runbook_source_changed");
    assert_eq!(mission.snapshot("journal"), journal);
    assert_eq!(mission.https.requests().len(), 12);
}

#[test]
fn journal_refuses_symlink_and_nonprivate_directory() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    for link in [false, true] {
        let (mission, inputs) = Mission::pair(metric("a", 0, json!([1, 1])));
        let journal = mission.https.directory.join("journal");
        if link {
            let other = mission.https.directory.join("elsewhere");
            fs::create_dir(&other).unwrap();
            fs::set_permissions(&other, fs::Permissions::from_mode(0o700)).unwrap();
            symlink(other, &journal).unwrap();
        } else {
            fs::create_dir(&journal).unwrap();
            fs::set_permissions(&journal, fs::Permissions::from_mode(0o755)).unwrap();
        }
        mission.run(&mut mission.start(&inputs), 2);
        assert!(fs::read_dir(&journal).unwrap().next().is_none());
        assert_eq!(mission.https.requests().len(), 12);
    }
}

#[test]
fn competing_resumes_cannot_acknowledge_the_same_revision_twice() {
    use std::process::Stdio;
    let (mission, inputs) = Mission::pair(metric("a", 0, json!([1, 1])));
    let waiting = mission.run(&mut mission.start(&inputs), 1);
    let mut a = mission.resume(&waiting, &inputs, Some("review-complete"));
    let mut b = mission.resume(&waiting, &inputs, Some("review-complete"));
    let a = a
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let b = b
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let outputs = [a.wait_with_output().unwrap(), b.wait_with_output().unwrap()];
    let receipts: Vec<_> = outputs.iter().map(https::receipt).collect();
    assert_eq!(
        outputs.iter().filter(|o| o.status.success()).count(),
        1,
        "{receipts:?}"
    );
    let completed = receipts
        .iter()
        .find(|r| r["data"]["workflow_state"] == "completed")
        .unwrap();
    assert_eq!(mission.status(completed)["data"], completed["data"]);
    assert_eq!(observations(completed), observations(&waiting));
    assert_eq!(mission.https.requests().len(), 12);
}

#[test]
fn busy_evidence_store_is_transient_and_preserves_the_waiting_checkpoint() {
    use std::os::fd::AsRawFd;
    let (mission, inputs) = Mission::pair(metric("a", 0, json!([1, 1])));
    let waiting = mission.run(&mut mission.start(&inputs), 1);
    let journal = mission.snapshot("journal");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(mission.https.directory.join("state/lock"))
        .unwrap();
    // SAFETY: the descriptor remains owned by lock until after explicit unlock.
    assert_eq!(
        // SAFETY: lock owns a live descriptor; flock neither dereferences pointers nor changes file bytes.
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let refused = mission.run(
        &mut mission.resume(&waiting, &inputs, Some("review-complete")),
        2,
    );
    error(&refused, "store_busy");
    assert!(
        !refused["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["code"] == "runbook_source_changed")
    );
    assert_eq!(mission.snapshot("journal"), journal);
    // SAFETY: this is the same live descriptor that acquired the lock above.
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) }, 0);
    drop(lock);
    let complete = mission.run(
        &mut mission.resume(&waiting, &inputs, Some("review-complete")),
        0,
    );
    assert_eq!(complete["data"]["workflow_state"], "completed");
    assert_eq!(observations(&complete), observations(&waiting));
    assert_eq!(mission.https.requests().len(), 12);
}

#[test]
fn uncertain_in_flight_crash_state_requires_fresh_attempt_without_replay() {
    let (mission, inputs) = Mission::pair(metric("a", 0, json!([1, 1])));
    let waiting = mission.run(&mut mission.start(&inputs), 1);
    let path = mission.https.directory.join("journal").join(format!(
        "{}.json",
        waiting["data"]["journal"]["id"].as_str().unwrap()
    ));
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    // This is a negative recovery fixture, not evidence for successful observations:
    // preserve the actual admitted identity and pins, then model death after the
    // durable before-step marker but before its result was retained.
    state["state"] = json!("running");
    state["next"] = json!(0);
    state["in_flight"] = json!("before");
    for step in state["steps"].as_array_mut().unwrap() {
        step["state"] = json!("pending");
        step["receipt"] = Value::Null;
        step["condition"] = Value::Null;
        step["acknowledged_at"] = Value::Null;
    }
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    let journal = mission.snapshot("journal");
    let evidence = mission.snapshot("state");
    let status = mission.status(&waiting);
    assert_eq!(status["data"]["workflow_state"], "running");
    assert_eq!(status["data"]["in_flight"], "before");
    assert_eq!(status["data"]["resume_safety"], "requires_fresh_attempt");
    assert!(observations(&status).is_empty());
    let refused = mission.run(&mut mission.resume(&waiting, &inputs, None), 2);
    error(&refused, "runbook_resume_unsafe");
    assert_eq!(mission.snapshot("journal"), journal);
    assert_eq!(mission.snapshot("state"), evidence);
    assert_eq!(mission.https.requests().len(), 12);
}

#[test]
fn stored_definition_drift_is_refused_before_checkpoint_mutation() {
    let (mission, inputs) = Mission::pair(metric("a", 0, json!([1, 1])));
    let waiting = mission.run(&mut mission.start(&inputs), 1);
    let path = mission.https.directory.join("journal").join(format!(
        "{}.json",
        waiting["data"]["journal"]["id"].as_str().unwrap()
    ));
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["definition"]["sha256"] = json!("0".repeat(64));
    fs::write(path, serde_json::to_vec(&state).unwrap()).unwrap();
    let journal = mission.snapshot("journal");
    let refused = mission.run(
        &mut mission.resume(&waiting, &inputs, Some("review-complete")),
        2,
    );
    error(&refused, "runbook_definition_mismatch");
    assert_eq!(mission.snapshot("journal"), journal);
    assert_eq!(mission.https.requests().len(), 12);
}
