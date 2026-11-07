#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};
use workbench_core::{
    EvidenceStoreOptions, OperatorContext, RunControl, execute_with_authority_identities,
    execute_with_investigation_grant, investigation_grant, normalize_investigation_input,
    request::Request,
    result::{Status, new_id},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(new_id("investigation"));
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        Self(p)
    }
    fn source(&self) -> PathBuf {
        let path = self.0.join("context.json");
        let mut value: Value = serde_json::from_slice(
            &fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/fixtures/context/unique.json"),
            )
            .unwrap(),
        )
        .unwrap();
        for record in value["records"].as_array_mut().unwrap() {
            record["last_reviewed"] = json!(workbench_core::result::timestamp());
        }
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        path
    }
    fn store(&self) -> PathBuf {
        let path = self.0.join("store");
        copy(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/evidence/evidence-comparison-2026-10-10/installed-r2/store"),
            &path,
        );
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn copy(from: &Path, to: &Path) {
    fs::create_dir(to).unwrap();
    fs::set_permissions(to, fs::Permissions::from_mode(0o700)).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dst = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &dst)
        } else {
            fs::copy(entry.path(), &dst).unwrap();
            fs::set_permissions(dst, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
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
fn operations(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}
fn selectors() -> Value {
    json!({"service":"checkout","environment":"dev"})
}
const BEFORE: &str = "run-9-18dd60cacef0724e-1";
const AFTER: &str = "run-16-18dd60cad194352b-1";

#[test]
fn generic_hosted_context_refuses_operator_source() {
    let f = Fixture::new();
    let context = OperatorContext {
        context_source_path: Some(f.source()),
        context_max_age_days: Some(30),
        ..Default::default()
    };
    let r = execute_with_authority_identities(
        Request::new("context.resolve", selectors()),
        &RunControl::default(),
        &context,
        None,
        None,
    );
    assert_eq!(r.execution.status, Status::Unsupported);
    assert_eq!(r.errors[0].code, "context_grants_unavailable");
}
#[test]
fn grant_resolves_exact_source_and_refuses_changed_bytes_or_inode() {
    let f = Fixture::new();
    let path = f.source();
    let mut context = OperatorContext {
        context_source_path: Some(path.clone()),
        context_max_age_days: Some(30),
        config_path: Some(f.0.join("never-open")),
        ..Default::default()
    };
    let g = investigation_grant(&context, &operations(&["context.resolve"])).unwrap();
    context.context_source_path = None;
    context.context_max_age_days = Some(1);
    assert!(g.allows("context.resolve"));
    assert!(!g.allows("process.exec"));
    let run = || {
        execute_with_investigation_grant(
            Request::new("context.resolve", selectors()),
            &RunControl::default(),
            &g,
        )
    };
    let r = run();
    assert_eq!(r.execution.status, Status::Succeeded, "{:?}", r.errors);
    assert_eq!(r.data["resolution"]["state"], "resolved");
    assert_eq!(r.data["resolution"]["max_age_days"], 30);
    let bytes = fs::read(&path).unwrap();
    fs::write(&path, [bytes.clone(), b" ".to_vec()].concat()).unwrap();
    let r = run();
    assert_eq!(r.errors[0].code, "context_source_changed");
    assert!(r.data.get("record").is_none());
    fs::rename(&path, f.0.join("old")).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert_eq!(run().errors[0].code, "context_source_changed");
    fs::remove_file(&path).unwrap();
    symlink(f.0.join("old"), &path).unwrap();
    assert_ne!(run().execution.status, Status::Succeeded);
}
#[test]
fn granted_historical_work_preserves_exact_store_bytes_and_rejects_replacement() {
    let f = Fixture::new();
    let path = f.store();
    let before = snapshot(&path);
    let context = OperatorContext {
        evidence_store: Some(EvidenceStoreOptions::new(path.clone())),
        ..Default::default()
    };
    let names = operations(&[
        "evidence.list",
        "evidence.open",
        "evidence.compare",
        "report.create",
    ]);
    let g = investigation_grant(&context, &names).unwrap();
    assert_eq!(snapshot(&path), before);
    for (operation, inputs) in [
        ("evidence.list", json!({})),
        ("evidence.open", json!({"id":BEFORE})),
        (
            "evidence.compare",
            json!({"before":{"id":BEFORE},"after":{"id":AFTER},"normalization":"exact-window-v1"}),
        ),
        (
            "report.create",
            json!({"bundles":[{"id":BEFORE},{"id":AFTER}],"format":"json"}),
        ),
    ] {
        let r = execute_with_investigation_grant(
            Request::new(operation, inputs),
            &RunControl::default(),
            &g,
        );
        assert!(
            matches!(r.execution.status, Status::Succeeded | Status::Partial),
            "{operation}: {:?}",
            r.errors
        );
        assert!(r.errors.is_empty(), "{operation}: {:?}", r.errors);
        assert_eq!(r.record_mode, "never");
        assert_eq!(snapshot(&path), before);
    }
    fs::rename(&path, f.0.join("old-store")).unwrap();
    copy(&f.0.join("old-store"), &path);
    let r = execute_with_investigation_grant(
        Request::new("evidence.list", json!({})),
        &RunControl::default(),
        &g,
    );
    assert_eq!(r.errors[0].code, "store_identity_changed");
    assert!(r.data.get("runs").is_none());
}
#[test]
fn grant_refuses_missing_bindings_unrelated_operations_and_bad_inputs() {
    let context = OperatorContext::default();
    for names in [
        vec![],
        operations(&["context.resolve"]),
        operations(&["evidence.list"]),
        operations(&["evidence.delete"]),
        operations(&["context.resolve", "context.resolve"]),
    ] {
        assert!(investigation_grant(&context, &names).is_err());
    }
    let f = Fixture::new();
    let context = OperatorContext {
        context_source_path: Some(f.source()),
        context_max_age_days: Some(30),
        ..Default::default()
    };
    let g = investigation_grant(&context, &operations(&["context.resolve"])).unwrap();
    for operation in [
        "process.exec",
        "evidence.list",
        "evidence.collect",
        "evidence.delete",
        "grafana.query",
        "task.run",
    ] {
        let r = execute_with_investigation_grant(
            Request::new(operation, json!({})),
            &RunControl::default(),
            &g,
        );
        assert_ne!(r.execution.status, Status::Succeeded);
        assert!(r.data.get("runs").is_none());
    }
    for (operation, value) in [
        (
            "context.resolve",
            json!({"service":"checkout","environment":"dev","source_path":"/tmp/x"}),
        ),
        (
            "context.resolve",
            json!({"service":"checkout","environment":"dev","team":null}),
        ),
        ("evidence.list", Value::Null),
        ("evidence.list", json!({"path":"/tmp/x"})),
        ("evidence.open", json!({"id":"../x"})),
        (
            "evidence.compare",
            json!({"before":{"id":BEFORE},"after":{"id":AFTER},"normalization":null}),
        ),
        (
            "report.create",
            json!({"bundles":[{"id":BEFORE}],"format":"json","path":"/tmp/x"}),
        ),
    ] {
        assert!(
            normalize_investigation_input(operation, &value).is_err(),
            "{operation}"
        );
    }
    let control = RunControl::default();
    control.signal.store(2, Ordering::Relaxed);
    let r = execute_with_investigation_grant(
        Request::new("context.resolve", selectors()),
        &control,
        &g,
    );
    assert_eq!(r.execution.status, Status::Cancelled);
    assert!(r.data.get("record").is_none());
    let mut request = Request::new("context.resolve", selectors());
    request.limits.timeout_ms = 0;
    assert_ne!(
        execute_with_investigation_grant(request, &RunControl::default(), &g)
            .execution
            .status,
        Status::Succeeded
    );
}

#[test]
fn startup_refuses_invalid_sources_absent_or_uninitialized_stores_without_creation() {
    let f = Fixture::new();
    let source = f.source();
    for bytes in [
        b"{}".as_slice(),
        b"{\"schema_version\":1,\"schema_version\":1}".as_slice(),
    ] {
        fs::write(&source, bytes).unwrap();
        let context = OperatorContext {
            context_source_path: Some(source.clone()),
            context_max_age_days: Some(30),
            ..Default::default()
        };
        assert!(investigation_grant(&context, &operations(&["context.resolve"])).is_err());
        assert_eq!(fs::read(&source).unwrap(), bytes);
    }
    fs::remove_file(&source).unwrap();
    let source = f.source();
    let link = f.0.join("link.json");
    symlink(&source, &link).unwrap();
    let context = OperatorContext {
        context_source_path: Some(link),
        context_max_age_days: Some(30),
        ..Default::default()
    };
    assert!(investigation_grant(&context, &operations(&["context.resolve"])).is_err());
    for age in [None, Some(0), Some(366)] {
        let context = OperatorContext {
            context_source_path: Some(source.clone()),
            context_max_age_days: age,
            ..Default::default()
        };
        assert!(investigation_grant(&context, &operations(&["context.resolve"])).is_err());
    }
    let absent = f.0.join("absent");
    let context = OperatorContext {
        evidence_store: Some(EvidenceStoreOptions::new(absent.clone())),
        ..Default::default()
    };
    assert!(investigation_grant(&context, &operations(&["evidence.list"])).is_err());
    assert!(!absent.exists());
    fs::create_dir(&absent).unwrap();
    fs::set_permissions(&absent, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(investigation_grant(&context, &operations(&["evidence.list"])).is_err());
    assert_eq!(fs::read_dir(&absent).unwrap().count(), 0);
}

#[test]
fn grant_retains_historical_limits_manifest_pins_and_symlink_refusal() {
    let f = Fixture::new();
    let path = f.store();
    let before = snapshot(&path);
    let context = OperatorContext {
        evidence_store: Some(EvidenceStoreOptions::new(path.clone())),
        ..Default::default()
    };
    let g = investigation_grant(
        &context,
        &operations(&["evidence.open", "report.create", "evidence.compare"]),
    )
    .unwrap();
    let mut request = Request::new("evidence.open", json!({"id":BEFORE}));
    request.limits.max_items = Some(1);
    request.limits.max_output_bytes = 1024;
    let result = execute_with_investigation_grant(request, &RunControl::default(), &g);
    assert_eq!(result.execution.status, Status::Partial);
    assert!(result.data["omitted_details"].as_u64().unwrap() > 0);
    assert_eq!(result.effective_limits.max_items, Some(1));
    assert_eq!(result.effective_limits.max_output_bytes, 1024);
    let result = execute_with_investigation_grant(
        Request::new(
            "evidence.open",
            json!({"id":BEFORE,"expected_manifest_sha256":format!("sha256:{}","0".repeat(64))}),
        ),
        &RunControl::default(),
        &g,
    );
    assert_eq!(result.errors[0].code, "manifest_digest_mismatch");
    assert!(result.data.get("manifest").is_none());
    let mut report = Request::new(
        "report.create",
        json!({"bundles":[{"id":BEFORE}],"format":"markdown"}),
    );
    report.limits.max_output_bytes = 1024;
    let result = execute_with_investigation_grant(report, &RunControl::default(), &g);
    assert_eq!(result.execution.status, Status::Partial);
    assert_eq!(result.data["rendering"]["state"], "omitted");
    assert!(result.data["rendering"]["content"].is_null());
    assert_eq!(snapshot(&path), before);
    fs::rename(&path, f.0.join("moved")).unwrap();
    symlink(f.0.join("moved"), &path).unwrap();
    for (operation, inputs) in [
        ("evidence.open", json!({"id":BEFORE})),
        (
            "report.create",
            json!({"bundles":[{"id":BEFORE}],"format":"json"}),
        ),
        (
            "evidence.compare",
            json!({"before":{"id":BEFORE},"after":{"id":AFTER},"normalization":"exact-window-v1"}),
        ),
    ] {
        let result = execute_with_investigation_grant(
            Request::new(operation, inputs),
            &RunControl::default(),
            &g,
        );
        assert!(!result.errors.is_empty());
        assert!(result.data.get("manifest").is_none());
        assert!(result.data.get("report").is_none());
    }
    assert_eq!(snapshot(&f.0.join("moved")), before);
}

#[test]
fn normalization_is_strict_and_canonicalizes_report_defaults() {
    let implicit = json!({"bundles":[{"id":BEFORE}],"format":"json"});
    let explicit = json!({"bundles":[{"id":BEFORE}],"format":"json","title":"Evidence handoff","proposals":[]});
    assert_eq!(
        normalize_investigation_input("report.create", &implicit).unwrap(),
        explicit
    );
    assert_eq!(
        normalize_investigation_input("report.create", &explicit).unwrap(),
        explicit
    );
    assert_eq!(
        normalize_investigation_input("context.resolve", &selectors()).unwrap(),
        selectors()
    );
    assert_eq!(
        normalize_investigation_input("evidence.replay", &json!({"id":BEFORE})).unwrap(),
        json!({"id":BEFORE})
    );
}

#[test]
fn replay_requires_its_own_grant_and_preserves_offline_profile_and_store() {
    let fixture = Fixture::new();
    let path = fixture.store();
    let original = snapshot(&path);
    let context = OperatorContext {
        evidence_store: Some(EvidenceStoreOptions::new(path.clone())),
        config_path: Some(fixture.0.join("must-not-read-config")),
        ..Default::default()
    };
    let request = || Request::new("evidence.replay", json!({"id":BEFORE}));
    let generic =
        execute_with_authority_identities(request(), &RunControl::default(), &context, None, None);
    assert_eq!(generic.errors[0].code, "evidence_grants_unavailable");
    let open = investigation_grant(&context, &operations(&["evidence.open"])).unwrap();
    let refused = execute_with_investigation_grant(request(), &RunControl::default(), &open);
    assert_eq!(
        refused.errors[0].code,
        "investigation_operation_not_granted"
    );

    let replay = investigation_grant(&context, &operations(&["evidence.replay"])).unwrap();
    let result = execute_with_investigation_grant(request(), &RunControl::default(), &replay);
    assert_eq!(
        result.execution.status,
        Status::Succeeded,
        "{:?}",
        result.errors
    );
    assert_eq!(result.data["mode"], "replay");
    assert_eq!(result.data["analysis"]["profile"], "linux-offline-v1");
    assert_eq!(result.data["algorithm"]["id"], "retained-observations-v1");
    assert_eq!(result.data["bundle"]["id"], BEFORE);
    assert_eq!(result.record_mode, "never");
    for operation in [
        "evidence.open",
        "evidence.collect",
        "evidence.delete",
        "grafana.query",
        "process.exec",
    ] {
        let denied = execute_with_investigation_grant(
            Request::new(operation, json!({})),
            &RunControl::default(),
            &replay,
        );
        assert_eq!(denied.errors[0].code, "investigation_operation_not_granted");
    }
    let control = RunControl::default();
    control.signal.store(2, Ordering::Relaxed);
    let cancelled = execute_with_investigation_grant(request(), &control, &replay);
    assert_eq!(cancelled.execution.status, Status::Cancelled);
    assert!(cancelled.data.get("checks").is_none());
    assert_eq!(snapshot(&path), original);
}
