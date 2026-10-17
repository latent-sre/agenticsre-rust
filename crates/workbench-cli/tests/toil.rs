#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const SAVE: &str = env!("CARGO_BIN_EXE_save");

fn utc(seconds: i64) -> String {
    let raw = seconds as libc::time_t;
    let mut result = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: both pointers are live and gmtime_r initializes the output on success.
    assert!(!unsafe { libc::gmtime_r(&raw, result.as_mut_ptr()) }.is_null());
    // SAFETY: successful gmtime_r initialized result above.
    let value = unsafe { result.assume_init() };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        value.tm_year + 1900,
        value.tm_mon + 1,
        value.tm_mday,
        value.tm_hour,
        value.tm_min,
        value.tm_sec
    )
}

struct Fixture {
    directory: PathBuf,
    now: i64,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(workbench_core::result::new_id("toil-test"));
        fs::create_dir(&directory).unwrap();
        Self {
            directory,
            now: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        }
    }
    fn command(&self, write: bool) -> Command {
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .args(["--json", "--toil-state-dir"])
            .arg(self.directory.join("state"));
        if write {
            command.args(["--record", "always"]);
        }
        command
    }
    fn run(&self, command: &mut Command, exit: i32) -> Value {
        let output = command.output().unwrap();
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(exit), "{result}");
        assert_eq!(result["assessment"], "not_assessed");
        result
    }
    fn invoke(&self, operation: &str, input: &Value, write: bool, exit: i32) -> Value {
        let path = self.directory.join("input.json");
        fs::write(&path, input.to_string()).unwrap();
        self.run(
            self.command(write)
                .args(["toil", operation, "--input"])
                .arg(path),
            exit,
        )
    }
    fn configure(&self, enabled: bool, days: u16) -> Value {
        self.run(
            self.command(true).args([
                "toil",
                "configure",
                "--enabled",
                if enabled { "true" } else { "false" },
                "--keep-days",
                &days.to_string(),
            ]),
            0,
        )
    }
    fn sample(&self, cohort: &str, effort: u64) -> Value {
        json!({"workflow":"dashboard-review","cohort":cohort,"observed_at":utc(self.now-3600),"elapsed_ms":effort+200,
            "effort_ms":effort,"measurement":"measured","outcome":"succeeded","retries":0,"manual_interventions":1})
    }
    fn summary(&self) -> Value {
        json!({"workflow":"dashboard-review","from":utc(self.now-7*86400),"to":utc(self.now+86400),
            "costs":{"build_ms":1000,"maintenance_ms":200,"incident_ms":300,"measurement":"measured"},
            "projection":{"runs_per_day":10,"horizon_days":30}})
    }
    fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, path: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            if !path.exists() {
                return;
            }
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
        let root = self.directory.join("state");
        let mut out = BTreeMap::new();
        walk(&root, &root, &mut out);
        out
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn collection_requires_explicit_consent_and_opt_out_preserves_readable_history() {
    let fixture = Fixture::new();
    fixture.run(fixture.command(true).args(["toil", "configure"]), 2);
    assert!(!fixture.directory.join("state").exists());
    fixture.configure(false, 30);
    let snapshot = fixture.snapshot();
    let refused = fixture.invoke("record", &fixture.sample("baseline", 1000), true, 2);
    assert_eq!(refused["effect_outcome"], "not_attempted");
    assert_eq!(fixture.snapshot(), snapshot);
    fixture.configure(true, 30);
    let recorded = fixture.invoke("record", &fixture.sample("baseline", 1000), true, 0);
    assert_eq!(recorded["data"]["sample"]["source"], "operator_reported");
    fixture.configure(false, 30);
    let snapshot = fixture.snapshot();
    fixture.invoke("record", &fixture.sample("candidate", 500), true, 2);
    let summary = fixture.invoke("summary", &fixture.summary(), false, 0);
    assert_eq!(summary["data"]["cohorts"]["baseline"]["samples"], 1);
    assert_eq!(
        summary["data"]["comparison"]["state"],
        "insufficient_samples"
    );
    assert!(summary["data"]["comparison"]["observed_net_ms"].is_null());
    assert_eq!(fixture.snapshot(), snapshot);
}

#[test]
fn known_effort_costs_frequency_and_break_even_have_independent_expected_values() {
    let fixture = Fixture::new();
    fixture.configure(true, 30);
    for (cohort, effort) in [
        ("baseline", 1000),
        ("baseline", 2000),
        ("candidate", 500),
        ("candidate", 1000),
    ] {
        fixture.invoke("record", &fixture.sample(cohort, effort), true, 0);
    }
    let snapshot = fixture.snapshot();
    let summary = fixture.invoke("summary", &fixture.summary(), false, 0);
    let data = &summary["data"];
    assert_eq!(data["cohorts"]["baseline"]["samples"], 2);
    assert_eq!(
        data["cohorts"]["baseline"]["effort"]["mean_ms"].as_f64(),
        Some(1500.0)
    );
    assert_eq!(
        data["cohorts"]["candidate"]["effort"]["mean_ms"].as_f64(),
        Some(750.0)
    );
    assert_eq!(
        data["cohorts"]["baseline"]["frequency"]["runs_per_day"].as_f64(),
        Some(0.25)
    );
    assert_eq!(data["costs"]["total_ms"], 1500);
    assert_eq!(data["comparison"]["gross_per_run_ms"].as_f64(), Some(750.0));
    assert_eq!(
        data["comparison"]["observed_gross_ms"].as_f64(),
        Some(1500.0)
    );
    assert_eq!(data["comparison"]["observed_net_ms"].as_f64(), Some(0.0));
    assert_eq!(data["comparison"]["break_even_runs"], 2);
    assert_eq!(data["projection"]["runs"], 300);
    assert_eq!(data["projection"]["net_ms"].as_f64(), Some(223500.0));
    assert_eq!(data["projection"]["measurement"], "estimated");
    assert!(!data["limitations"].as_array().unwrap().is_empty());
    assert_eq!(fixture.snapshot(), snapshot);
}

#[test]
fn task_capture_retains_no_input_and_effort_is_unknown_until_annotated() {
    let fixture = Fixture::new();
    fixture.configure(true, 30);
    fixture.invoke("record", &fixture.sample("baseline", 1000), true, 0);
    let input = json!({"workflow":"dashboard-review","cohort":"candidate","task":{"id":"error-budget","version":1,"input":{"slo":99.912345,"bad_minutes":0}}});
    let captured = fixture.invoke("capture", &input, true, 0);
    assert_eq!(captured["data"]["child"]["operation"], "task.run");
    assert_eq!(
        captured["data"]["child"]["execution"]["status"],
        "succeeded"
    );
    assert_eq!(captured["data"]["recording"]["state"], "complete");
    let sample = &captured["data"]["sample"];
    assert_eq!(sample["source"], "task_capture");
    assert_eq!(sample["elapsed_measurement"], "measured");
    assert!(sample["effort_ms"].is_null());
    for bytes in fixture.snapshot().values() {
        let text = String::from_utf8_lossy(bytes);
        for forbidden in [
            "99.912345",
            "bad_minutes",
            "calculation",
            "stdout",
            "stderr",
        ] {
            assert!(!text.contains(forbidden), "persisted {forbidden}");
        }
    }
    let unknown = fixture.invoke("summary", &fixture.summary(), false, 0);
    assert_eq!(unknown["data"]["comparison"]["state"], "missing_effort");
    assert!(unknown["data"]["comparison"]["observed_net_ms"].is_null());
    fixture.invoke("annotate",&json!({"id":sample["id"],"effort_ms":250,"measurement":"estimated","manual_interventions":0}),true,0);
    let annotated = fixture.invoke("summary", &fixture.summary(), false, 0);
    assert_eq!(annotated["data"]["cohorts"]["candidate"]["samples"], 1);
    assert_eq!(
        annotated["data"]["cohorts"]["candidate"]["effort"]["estimated"],
        1
    );
    assert_eq!(
        annotated["data"]["comparison"]["input_measurement"],
        "estimated"
    );
}

#[test]
fn retention_excludes_expired_rows_then_prune_and_delete_remove_only_owned_samples() {
    let fixture = Fixture::new();
    fixture.configure(true, 30);
    let mut old = fixture.sample("baseline", 1000);
    old["observed_at"] = json!(utc(fixture.now - 2 * 86400));
    let old = fixture.invoke("record", &old, true, 0);
    let young = fixture.invoke("record", &fixture.sample("candidate", 500), true, 0);
    fixture.configure(false, 1);
    let snapshot = fixture.snapshot();
    let summary = fixture.invoke("summary", &fixture.summary(), false, 0);
    assert_eq!(summary["data"]["expired_samples_excluded"], 1);
    assert_eq!(summary["data"]["cohorts"]["baseline"]["samples"], 0);
    assert_eq!(summary["data"]["cohorts"]["candidate"]["samples"], 1);
    assert_eq!(fixture.snapshot(), snapshot);
    let pruned = fixture.run(fixture.command(true).args(["toil", "prune"]), 0);
    assert_eq!(
        pruned["data"]["deleted"],
        json!([old["data"]["sample"]["id"]])
    );
    let deleted = fixture.run(
        fixture.command(true).args([
            "toil",
            "delete",
            young["data"]["sample"]["id"].as_str().unwrap(),
        ]),
        0,
    );
    assert_eq!(deleted["data"]["remaining"], 0);
}

#[test]
fn invalid_metadata_and_capture_are_refused_without_mutation_or_child() {
    let fixture = Fixture::new();
    fixture.configure(true, 30);
    let snapshot = fixture.snapshot();
    for (key, value) in [
        ("command", json!("touch marker")),
        ("person", json!("employee")),
        ("effort_ms", Value::Null),
        ("measurement", json!("verified")),
        ("observed_at", json!(utc(fixture.now + 86400))),
        ("elapsed_ms", json!(31536000001_u64)),
    ] {
        let mut invalid = fixture.sample("baseline", 1000);
        invalid[key] = value;
        fixture.invoke("record", &invalid, true, 2);
    }
    let invalid = json!({"workflow":"dashboard-review","cohort":"candidate","task":{"id":"custom-shell","version":1,"input":{}}});
    fixture.invoke("capture", &invalid, true, 2);
    fixture.invoke("capture",&json!({"workflow":"dashboard-review","cohort":"candidate","task":{"id":"error-budget","version":1,"input":{"slo":101}}}),true,2);
    assert_eq!(snapshot, fixture.snapshot());
}
