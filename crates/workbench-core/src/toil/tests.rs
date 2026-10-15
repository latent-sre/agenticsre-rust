use super::*;
use crate::{
    execute_with_authority_identities, execute_with_context,
    result::{Status, new_id},
};
fn sample(id: usize, cohort: &str, effort: Option<u64>) -> Sample {
    Sample {
        id: format!("sample-1-a-{id:x}"),
        workflow: "deploy.check".into(),
        cohort: cohort.into(),
        observed_at: "2026-10-11T00:00:00Z".into(),
        elapsed_ms: Some(10),
        elapsed_measurement: Some("measured".into()),
        effort_ms: effort,
        effort_measurement: effort.map(|_| "measured".into()),
        effort_provenance: effort.map(|_| "operator_reported".into()),
        outcome: "succeeded".into(),
        retries: Some(0),
        manual_interventions: Some(0),
        source: "operator_reported".into(),
        task: None,
        operation_run_id: None,
        annotated_at: None,
    }
}
fn query() -> Value {
    json!({"workflow":"deploy.check","from":"2026-10-10T00:00:00Z","to":"2026-10-12T00:00:00Z","costs":{"build_ms":1000,"maintenance_ms":300,"incident_ms":200,"measurement":"measured"},"projection":{"runs_per_day":10,"horizon_days":30}})
}
fn summarize(samples: &[Sample], input: &Value) -> Value {
    summary::build(
        input,
        samples,
        true,
        30,
        utc(&json!("2026-10-11T12:00:00Z")).unwrap(),
        &mut || Ok(()),
    )
    .unwrap()
}
#[test]
fn hand_calculated_labor_savings_include_all_costs_and_failed_episodes() {
    let mut samples = vec![
        sample(0, "baseline", Some(1000)),
        sample(1, "baseline", Some(2000)),
        sample(2, "candidate", Some(500)),
        sample(3, "candidate", Some(1000)),
    ];
    samples[3].outcome = "failed".into();
    let r = summarize(&samples, &query());
    assert_eq!(r["comparison"]["gross_per_run_ms"], 750.0);
    assert_eq!(r["comparison"]["observed_gross_ms"], 1500.0);
    assert_eq!(r["comparison"]["observed_net_ms"], 0.0);
    assert_eq!(r["comparison"]["break_even_runs"], 2);
    assert_eq!(r["projection"]["runs"], 300);
    assert_eq!(r["projection"]["net_ms"], 223500.0);
    assert_eq!(r["projection"]["measurement"], "estimated");
    assert_eq!(r["cohorts"]["candidate"]["samples"], 2);
    assert_eq!(r["cohorts"]["candidate"]["outcomes"]["failed"], 1);
}
#[test]
fn missing_effort_is_not_runtime_and_negative_benefit_never_has_break_even() {
    let mut samples = vec![sample(0, "baseline", Some(1)), sample(1, "candidate", None)];
    let r = summarize(&samples, &query());
    assert_eq!(r["comparison"]["state"], "missing_effort");
    assert!(r["comparison"]["gross_per_run_ms"].is_null());
    assert_eq!(r["cohorts"]["candidate"]["effort"]["known"], 0);
    assert_eq!(r["cohorts"]["candidate"]["elapsed"]["known"], 1);
    samples[1].effort_ms = Some(2);
    samples[1].effort_measurement = Some("estimated".into());
    samples[1].effort_provenance = Some("operator_reported".into());
    let r = summarize(&samples, &query());
    assert_eq!(r["comparison"]["gross_per_run_ms"], -1.0);
    assert_eq!(r["comparison"]["break_even_state"], "no_positive_benefit");
    assert!(r["comparison"]["break_even_runs"].is_null());
    assert_eq!(r["comparison"]["input_measurement"], "estimated");
}
#[test]
fn rational_break_even_does_not_round_fractional_savings_and_large_projection_stays_finite() {
    let mut samples: Vec<_> = (0..1023)
        .map(|i| sample(i, "baseline", Some(u64::from(i == 0))))
        .collect();
    samples.push(sample(1023, "candidate", Some(0)));
    let mut q = query();
    q["costs"] = json!({"build_ms":YEAR_MS,"maintenance_ms":YEAR_MS,"incident_ms":YEAR_MS,"measurement":"measured"});
    q["projection"] = json!({"runs_per_day":MAX_COUNTER,"horizon_days":3650});
    let r = summarize(&samples, &q);
    assert_eq!(r["comparison"]["break_even_runs"], 3 * YEAR_MS * 1023);
    assert!(r["projection"]["net_ms"].as_f64().unwrap().is_finite());
    assert!(serde_json::to_string(&r).is_ok());
}
#[test]
fn half_open_nanosecond_selection_expiry_and_cancellation_remain_explicit() {
    let mut samples = vec![
        sample(0, "baseline", Some(1)),
        sample(1, "candidate", Some(0)),
        sample(2, "candidate", Some(9)),
    ];
    samples[0].observed_at = "2026-10-11T00:00:00.000000001Z".into();
    samples[1].observed_at = "2026-10-11T00:00:00.001000000Z".into();
    samples[2].observed_at = "2026-10-11T00:00:00.001000001Z".into();
    let mut q = query();
    q["from"] = json!("2026-10-11T00:00:00.000000001Z");
    q["to"] = json!("2026-10-11T00:00:00.001000001Z");
    assert!(validate("toil.summary", &q).is_ok());
    let r = summarize(&samples, &q);
    assert_eq!(r["cohorts"]["candidate"]["samples"], 1);
    assert_eq!(r["cohorts"]["baseline"]["frequency"]["window_ms"], 1);
    q["to"] = json!("2026-10-11T00:00:00.000000002Z");
    assert!(validate("toil.summary", &q).is_err());
    assert_eq!(
        summary::build(
            &query(),
            &samples,
            true,
            30,
            OffsetDateTime::now_utc(),
            &mut || Err("toil_cancelled")
        )
        .err(),
        Some("toil_cancelled")
    );
}
#[test]
fn privacy_schema_refuses_person_raw_content_nulls_and_nonlexical_utc() {
    let good = json!({"workflow":"deploy.check","cohort":"baseline","observed_at":"2026-10-11T00:00:00Z","measurement":"measured","outcome":"succeeded"});
    assert!(validate("toil.record", &good).is_ok());
    for (key, value) in [
        ("person", json!("someone")),
        ("command", json!("echo secret")),
        ("effort_ms", Value::Null),
        ("elapsed_ms", json!(YEAR_MS + 1)),
        ("observed_at", json!("2026-10-11t00:00:00Z")),
        ("observed_at", json!("2026-10-11T00:00:00.1234567890Z")),
    ] {
        let mut input = good.clone();
        input[key] = value;
        assert!(validate("toil.record", &input).is_err(), "{key}");
    }
    let mut request = Request::new("toil.record", good);
    assert_eq!(request.validate().unwrap_err().code, "recording_required");
    request.record = "auto".into();
    assert!(request.validate().is_ok());
}
#[test]
fn hosted_requests_never_inherit_private_toil_consent() {
    let dir = std::env::temp_dir().join(new_id("absent-toil"));
    let context = OperatorContext {
        toil_state_dir: Some(dir.clone()),
        ..Default::default()
    };
    for op in [
        "toil.configure",
        "toil.record",
        "toil.capture",
        "toil.annotate",
        "toil.summary",
        "toil.prune",
        "toil.delete",
    ] {
        let mut request = Request::new(op, json!({}));
        if op != "toil.summary" {
            request.record = "auto".into();
        }
        let result = execute_with_authority_identities(
            request,
            &RunControl::default(),
            &context,
            None,
            None,
        );
        assert_eq!(result.errors[0].code, "toil_grants_unavailable");
        assert!(!dir.exists());
    }
}
#[cfg(target_os = "linux")]
mod storage {
    use super::*;
    use crate::toil::store::{FAIL_BEFORE_PUBLISH, Store};
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        path::PathBuf,
        time::Duration,
    };
    struct Fixture {
        base: PathBuf,
        context: OperatorContext,
    }
    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(new_id("toil-test"));
            fs::create_dir(&base).unwrap();
            fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
            Self {
                context: OperatorContext {
                    toil_state_dir: Some(base.join("metadata")),
                    ..Default::default()
                },
                base,
            }
        }
        fn call(&self, op: &str, input: Value) -> OperationResult {
            let mut request = Request::new(op, input);
            if op != "toil.summary" {
                request.record = "auto".into();
            }
            execute_with_context(request, &RunControl::default(), &self.context)
        }
        fn state(&self) -> PathBuf {
            self.context
                .toil_state_dir
                .as_ref()
                .unwrap()
                .join("state.json")
        }
        fn configure(&self, enabled: bool, days: u64) {
            let result = self.call(
                "toil.configure",
                json!({"enabled":enabled,"retention_days":days}),
            );
            assert_eq!(
                result.execution.status,
                Status::Succeeded,
                "{:?}",
                result.errors
            );
        }
        fn record(&self, observed_at: String) -> OperationResult {
            self.call("toil.record",json!({"workflow":"deploy.check","cohort":"baseline","observed_at":observed_at,"effort_ms":100,"measurement":"measured","outcome":"succeeded"}))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }
    #[test]
    fn consent_annotation_retention_and_deletion_preserve_single_episode() {
        let f = Fixture::new();
        let r = f.record(crate::result::timestamp());
        assert_ne!(r.execution.status, Status::Succeeded);
        assert!(!f.state().exists());
        f.configure(true, 30);
        let old = (OffsetDateTime::now_utc() - time::Duration::days(2))
            .format(&Rfc3339)
            .unwrap();
        let r = f.record(old);
        assert_eq!(r.execution.status, Status::Succeeded);
        let oldid = r.data["sample"]["id"].clone();
        let r = f.record(crate::result::timestamp());
        let id = r.data["sample"]["id"].clone();
        let r = f.call(
            "toil.annotate",
            json!({"id":id,"effort_ms":200,"measurement":"estimated"}),
        );
        assert_eq!(r.data["sample"]["effort_provenance"], "operator_annotated");
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(f.state()).unwrap()).unwrap()["samples"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        f.configure(false, 1);
        let before = fs::read(f.state()).unwrap();
        assert_eq!(
            f.record(crate::result::timestamp()).errors[0].code,
            "toil_disabled"
        );
        assert_eq!(
            f.call(
                "toil.annotate",
                json!({"id":id,"effort_ms":1,"measurement":"measured"})
            )
            .errors[0]
                .code,
            "toil_disabled"
        );
        assert_eq!(fs::read(f.state()).unwrap(), before);
        let prune = f.call("toil.prune", json!({}));
        assert_eq!(prune.data["deleted"], json!([oldid]));
        assert_eq!(prune.data["remaining"], 1);
        let deleted = f.call("toil.delete", json!({"id":id}));
        assert_eq!(deleted.data["remaining"], 0);
    }
    #[test]
    fn atomic_failure_preserves_old_bytes_and_unknown_formats_are_not_rewritten() {
        let f = Fixture::new();
        f.configure(true, 30);
        let before = fs::read(f.state()).unwrap();
        FAIL_BEFORE_PUBLISH.with(|flag| flag.set(true));
        let failed = f.record(crate::result::timestamp());
        assert_eq!(failed.errors[0].code, "toil_test_write_failed");
        assert_eq!(fs::read(f.state()).unwrap(), before);
        assert_eq!(
            fs::read_dir(f.state().parent().unwrap()).unwrap().count(),
            2
        );
        let mut value: Value = serde_json::from_slice(&before).unwrap();
        value["format_version"] = json!(99);
        fs::write(f.state(), serde_json::to_vec(&value).unwrap()).unwrap();
        let unknown = fs::read(f.state()).unwrap();
        assert_eq!(
            f.call(
                "toil.configure",
                json!({"enabled":false,"retention_days":7})
            )
            .errors[0]
                .code,
            "toil_unsupported_format"
        );
        assert_eq!(fs::read(f.state()).unwrap(), unknown);
    }
    #[test]
    fn capture_arrays_refuse_before_store_or_task_and_failed_record_keeps_child() {
        let f = Fixture::new();
        for task in [
            json!(["error-budget",1,{"slo":99.9,"bad_minutes":0}]),
            json!({"id":"dashboard-hygiene","version":1,"input":["/tmp/model.json"]}),
        ] {
            let result = f.call(
                "toil.capture",
                json!({"workflow":"deploy.check","cohort":"candidate","task":task}),
            );
            assert_eq!(result.errors[0].code, "invalid_task_input");
            assert!(result.data["child"].is_null());
            assert!(!f.state().parent().unwrap().exists());
        }
        f.configure(true, 30);
        let before = fs::read(f.state()).unwrap();
        FAIL_BEFORE_PUBLISH.with(|flag| flag.set(true));
        let result=f.call("toil.capture",json!({"workflow":"deploy.check","cohort":"candidate","task":{"id":"error-budget","version":1,"input":{"slo":99.9,"bad_minutes":0}}}));
        assert_eq!(
            result.data["child"]["execution"]["status"], "succeeded",
            "{:?}",
            result.errors
        );
        assert_eq!(result.execution.status, Status::Failed);
        assert_eq!(result.data["recording"]["state"], "failed");
        assert!(result.data["sample"].is_null());
        assert_eq!(result.errors[0].code, "toil_test_write_failed");
        assert_eq!(fs::read(f.state()).unwrap(), before);
        assert_eq!(
            fs::read_dir(f.state().parent().unwrap()).unwrap().count(),
            2
        );
    }

    #[test]
    fn cancellation_and_deadline_before_publication_preserve_old_state() {
        let f = Fixture::new();
        f.configure(true, 30);
        let before = fs::read(f.state()).unwrap();
        let mut store = Store::open(
            f.context.toil_state_dir.as_ref().unwrap(),
            false,
            &RunControl::default(),
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap();
        store.state.enabled = false;
        let control = RunControl::default();
        control
            .signal
            .store(2, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            store
                .commit(&control, Instant::now() + Duration::from_secs(30))
                .unwrap_err()
                .code,
            "toil_cancelled"
        );
        assert_eq!(fs::read(f.state()).unwrap(), before);
        assert_eq!(
            store
                .commit(&RunControl::default(), Instant::now())
                .unwrap_err()
                .code,
            "toil_deadline_exceeded"
        );
        assert_eq!(fs::read(f.state()).unwrap(), before);
    }

    #[test]
    fn lock_only_and_array_encoded_samples_are_preserved_and_refused() {
        let f = Fixture::new();
        f.configure(true, 30);
        let initial = fs::read(f.state()).unwrap();
        let mut value: Value = serde_json::from_slice(&initial).unwrap();
        value["samples"] = json!([[]]);
        fs::write(f.state(), serde_json::to_vec(&value).unwrap()).unwrap();
        let before = fs::read(f.state()).unwrap();
        assert_eq!(
            f.call(
                "toil.configure",
                json!({"enabled":false,"retention_days":7})
            )
            .errors[0]
                .code,
            "toil_store_corrupt"
        );
        assert_eq!(fs::read(f.state()).unwrap(), before);
        fs::remove_file(f.state()).unwrap();
        let result = f.call("toil.configure", json!({"enabled":true,"retention_days":7}));
        assert_eq!(result.errors[0].code, "toil_unconfigured");
        assert!(!f.state().exists());
    }

    #[test]
    fn unsafe_entries_lock_contention_and_full_quota_refuse_before_capture() {
        let f = Fixture::new();
        f.configure(true, 30);
        let path = f.context.toil_state_dir.as_ref().unwrap();
        let mut store = Store::open(
            path,
            false,
            &RunControl::default(),
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(
            f.call(
                "toil.configure",
                json!({"enabled":false,"retention_days":7})
            )
            .errors[0]
                .code,
            "store_busy"
        );
        store.state.samples = (0..1024)
            .map(|i| {
                let mut s = sample(i, "baseline", None);
                s.observed_at = crate::result::timestamp();
                s
            })
            .collect();
        store
            .commit(
                &RunControl::default(),
                Instant::now() + Duration::from_secs(30),
            )
            .unwrap();
        drop(store);
        let before = fs::read(f.state()).unwrap();
        let r=f.call("toil.capture",json!({"workflow":"deploy.check","cohort":"candidate","task":{"id":"error-budget","version":1,"input":{"slo":99.9,"bad_minutes":0}}}));
        assert_eq!(r.errors[0].code, "toil_quota_exceeded");
        assert!(r.data["child"].is_null());
        assert_eq!(fs::read(f.state()).unwrap(), before);
        symlink("state.json", path.join("unrecognized")).unwrap();
        assert_eq!(
            f.call("toil.prune", json!({})).errors[0].code,
            "toil_unknown_layout"
        );
        assert_eq!(fs::read(f.state()).unwrap(), before);
    }
}
