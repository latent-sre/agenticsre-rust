use super::*;
use crate::{request::Request, result::OperationResult};
use serde_json::json;
use std::{
    fs as stdfs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    time::Duration,
};

struct Fixture {
    base: PathBuf,
    options: EvidenceStoreOptions,
}

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(crate::result::new_id("evidence-store-test"));
        stdfs::create_dir(&base).unwrap();
        stdfs::set_permissions(&base, stdfs::Permissions::from_mode(0o700)).unwrap();
        Self {
            options: EvidenceStoreOptions::new(base.join("store")),
            base,
        }
    }

    fn open(&self, create: bool) -> Result<Store, StoreError> {
        Store::open(
            &self.options,
            create,
            Instant::now() + Duration::from_secs(30),
        )
    }

    fn run(&self, id: &str) -> PathBuf {
        self.options.directory.join("runs").join(id)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        FAILURE.with(|failure| failure.set(None));
        CANCEL_AFTER_PROGRESS.with(|limit| limit.set(None));
        let _ = stdfs::remove_dir_all(&self.base);
    }
}

fn receipt(data: Value) -> Vec<u8> {
    let mut request = Request::new("query.run", json!({}));
    request.target.kind = "connection".into();
    request.target.id = "fixture".into();
    let mut result = OperationResult::new(&request);
    result.data = data;
    let mut bytes = serde_json::to_vec(&result).unwrap();
    bytes.push(b'\n');
    bytes
}

fn sealed(store: &mut Store, bytes: &[u8]) -> (String, BundleRef, ArtifactRef) {
    let id = crate::result::new_id("run");
    let mut lease = store
        .begin_collection(&id, json!({"complete":false}), 128 * 1024)
        .unwrap();
    let artifact = lease.put_receipt("metrics", bytes).unwrap();
    let reference = lease.seal(json!({"complete":true})).unwrap();
    (id, reference, artifact)
}

fn edit_json(path: &Path, edit: impl FnOnce(&mut Value)) {
    let mut value: Value = serde_json::from_slice(&stdfs::read(path).unwrap()).unwrap();
    edit(&mut value);
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    stdfs::write(path, bytes).unwrap();
}

fn code<T>(result: Result<T, StoreError>) -> &'static str {
    match result {
        Err(error) => error.code,
        Ok(_) => panic!("expected a refusal"),
    }
}

#[test]
fn relative_store_options_are_rejected_before_access() {
    for directory in ["store", ".", "../store"] {
        let options = EvidenceStoreOptions::new(directory.into());
        assert_eq!(code(options.validate()), "store_invalid_options");
    }
}

#[test]
fn pre_intent_failures_can_be_explicitly_recovered_after_restart() {
    for point in [
        FailurePoint::Write,
        FailurePoint::FileSync,
        FailurePoint::Publish,
    ] {
        let fixture = Fixture::new();
        let id = crate::result::new_id("run");
        let mut store = fixture.open(true).unwrap();
        FAILURE.with(|failure| failure.set(Some((point, 0))));
        assert_eq!(
            code(store.begin_collection(&id, json!({}), 128 * 1024)),
            "store_injected_failure"
        );
        drop(store);
        let mut store = fixture.open(false).unwrap();
        assert!(fixture.run(&id).exists());
        assert_eq!(store.list().unwrap()[0].state, "incomplete");
        assert!(store.recover().unwrap().is_empty());
        assert!(!fixture.run(&id).exists());
        assert_eq!(store.progress().deleted, vec![id.clone()]);
        drop(store.begin_collection(&id, json!({}), 128 * 1024).unwrap());
    }
}

#[test]
fn empty_pre_intent_directories_are_removed_only_by_explicit_recovery() {
    for artifacts in [false, true] {
        let fixture = Fixture::new();
        let mut store = fixture.open(true).unwrap();
        let id = crate::result::new_id("run");
        let run = store.runs.create_dir(&id).unwrap();
        if artifacts {
            run.create_dir("artifacts").unwrap();
        }
        assert_eq!(store.list().unwrap()[0].state, "incomplete");
        assert!(store.prune(u64::MAX).unwrap().is_empty());
        assert!(fixture.run(&id).exists());
        assert!(store.recover().unwrap().is_empty());
        assert!(!fixture.run(&id).exists());
    }
}

#[test]
fn pre_intent_cleanup_sync_failures_remain_restartable() {
    for countdown in [0, 1] {
        let fixture = Fixture::new();
        let mut store = fixture.open(true).unwrap();
        let id = crate::result::new_id("run");
        FAILURE.with(|failure| failure.set(Some((FailurePoint::Write, 0))));
        assert_eq!(
            code(store.begin_collection(&id, json!({}), 128 * 1024)),
            "store_injected_failure"
        );
        drop(store);
        let mut store = fixture.open(false).unwrap();
        FAILURE.with(|failure| failure.set(Some((FailurePoint::DirectorySync, countdown))));
        let error = store.recover().unwrap_err();
        assert_eq!(error.code, "store_injected_failure");
        assert!(error.effects);
        drop(store);
        let mut store = fixture.open(false).unwrap();
        assert!(store.recover().unwrap().is_empty());
        assert_eq!(store.progress().deleted, vec![id.clone()]);
        assert!(!fixture.run(&id).exists());
    }
}

#[test]
fn sealed_unreferenced_artifacts_are_refused_by_reads_and_bulk_management() {
    let fixture = Fixture::new();
    let mut store = fixture.open(true).unwrap();
    let (id, _, _) = sealed(&mut store, &receipt(json!({"retained":true})));
    let extra = receipt(json!({"unreferenced":true}));
    let run = store.runs.open_at(&id, true).unwrap();
    let objects = run.open_at("artifacts", true).unwrap();
    objects
        .create_file(&format!("{}.json", sha256(&extra)))
        .unwrap()
        .write_all(&extra)
        .unwrap();
    assert_eq!(
        code(store.load_verified(&id)),
        "store_unreferenced_artifact"
    );
    assert_eq!(store.list().unwrap()[0].state, "corrupt");
    let before = snapshot_tree(&fixture.options.directory);
    assert_eq!(code(store.recover()), "store_unreferenced_artifact");
    assert_eq!(code(store.prune(u64::MAX)), "store_unreferenced_artifact");
    assert_eq!(snapshot_tree(&fixture.options.directory), before);
    store.delete(&id).unwrap();
    assert!(!fixture.run(&id).exists());
}

#[test]
fn pre_intent_recovery_preflights_unknown_formats_and_noninitial_layouts() {
    for kind in [
        "future",
        "checkpoint",
        "artifact",
        "mismatched",
        "protected",
    ] {
        let fixture = Fixture::new();
        let mut store = fixture.open(true).unwrap();
        let clean = crate::result::new_id("run");
        store.runs.create_dir(&clean).unwrap();
        let id = crate::result::new_id("run");
        let run = store.runs.create_dir(&id).unwrap();
        let objects = run.create_dir("artifacts").unwrap();
        match kind {
            "future" => run
                .create_file(".tmp-1-2-3")
                .unwrap()
                .write_all(b"{\"format_version\":987}")
                .unwrap(),
            "checkpoint" => run
                .create_file("checkpoint.json")
                .unwrap()
                .write_all(
                    &json_bytes(&Checkpoint {
                        format_version: 1,
                        id: id.clone(),
                        artifacts: BTreeMap::new(),
                        payload: json!({}),
                    })
                    .unwrap(),
                )
                .unwrap(),
            "artifact" => {
                let bytes = receipt(json!({}));
                objects
                    .create_file(&format!("{}.json", sha256(&bytes)))
                    .unwrap()
                    .write_all(&bytes)
                    .unwrap();
            }
            "mismatched" => run
                .create_file(".tmp-1-2-3")
                .unwrap()
                .write_all(
                    &json_bytes(&Intent {
                        format_version: 1,
                        id: clean.clone(),
                        created_at_unix_ms: 0,
                        reservation: 1024,
                        payload: json!({}),
                    })
                    .unwrap(),
                )
                .unwrap(),
            "protected" => run
                .create_file("protection.json")
                .unwrap()
                .write_all(
                    &json_bytes(&Protection {
                        format_version: 1,
                        id: id.clone(),
                        protected: true,
                    })
                    .unwrap(),
                )
                .unwrap(),
            _ => unreachable!(),
        }
        let before = snapshot_tree(&fixture.options.directory);
        assert!(store.recover().is_err(), "{kind}");
        assert!(store.delete(&id).is_err(), "{kind}");
        assert!(fixture.run(&clean).exists());
        assert!(fixture.run(&id).exists());
        assert_eq!(snapshot_tree(&fixture.options.directory), before);
    }
}

#[test]
fn invalid_pre_intent_staging_is_listed_without_hiding_valid_neighbors() {
    for (format_version, expected) in [(987, "store_unsupported_format"), (1, "store_corrupt")] {
        let fixture = Fixture::new();
        let mut store = fixture.open(true).unwrap();
        let (valid, _, _) = sealed(&mut store, &receipt(json!({})));
        let id = crate::result::new_id("run");
        let run = store.runs.create_dir(&id).unwrap();
        run.create_dir("artifacts").unwrap();
        run.create_file(".tmp-1-2-3")
            .unwrap()
            .write_all(
                &json_bytes(&Intent {
                    format_version,
                    id: valid.clone(),
                    created_at_unix_ms: 0,
                    reservation: 1024,
                    payload: json!({}),
                })
                .unwrap(),
            )
            .unwrap();
        let before = snapshot_tree(&fixture.options.directory);
        let summaries = store.list().unwrap();
        assert_eq!(summaries.len(), 2);
        let valid_summary = summaries
            .iter()
            .find(|summary| summary.id == valid)
            .unwrap();
        assert_eq!(valid_summary.state, "sealed");
        assert_eq!(valid_summary.error, None);
        let corrupt = summaries.iter().find(|summary| summary.id == id).unwrap();
        assert_eq!(corrupt.state, "corrupt");
        assert_eq!(corrupt.error, Some(expected));
        assert_eq!(snapshot_tree(&fixture.options.directory), before);
    }
}

#[test]
fn explicit_deletion_still_refuses_corrupt_or_protected_extra_artifacts() {
    for protected in [false, true] {
        let fixture = Fixture::new();
        let mut store = fixture.open(true).unwrap();
        let (id, _, _) = sealed(&mut store, &receipt(json!({})));
        if protected {
            store.protect(&id, true).unwrap();
        }
        let objects = store
            .runs
            .open_at(&id, true)
            .unwrap()
            .open_at("artifacts", true)
            .unwrap();
        let bytes = receipt(json!({"extra":true}));
        let name = if protected {
            sha256(&bytes)
        } else {
            "0".repeat(64)
        };
        objects
            .create_file(&format!("{name}.json"))
            .unwrap()
            .write_all(&bytes)
            .unwrap();
        let before = snapshot_tree(&fixture.options.directory);
        assert_eq!(
            code(store.delete(&id)),
            if protected {
                "store_protected"
            } else {
                "store_corrupt"
            }
        );
        assert_eq!(snapshot_tree(&fixture.options.directory), before);
    }
}

#[test]
fn public_recovery_discloses_removed_pre_intent_runs_without_fabricated_bundles() {
    let fixture = Fixture::new();
    let store = fixture.open(true).unwrap();
    let id = crate::result::new_id("run");
    store.runs.create_dir(&id).unwrap();
    drop(store);
    let context = crate::OperatorContext {
        evidence_store: Some(fixture.options.clone()),
        ..Default::default()
    };
    let result = crate::execute_with_context(
        Request::new("evidence.recover", json!({})),
        &crate::RunControl::default(),
        &context,
    );
    assert_eq!(
        result.execution.status,
        crate::result::Status::Succeeded,
        "{result:?}"
    );
    assert_eq!(result.data, json!({"recovered":[], "deleted":[id]}));
    assert!(!fixture.run(&id).exists());
}

#[test]
fn cancelled_public_pre_intent_recovery_reports_only_completed_deletions() {
    let fixture = Fixture::new();
    let store = fixture.open(true).unwrap();
    let mut ids = [crate::result::new_id("run"), crate::result::new_id("run")];
    ids.sort();
    for id in &ids {
        store.runs.create_dir(id).unwrap();
    }
    drop(store);
    CANCEL_AFTER_PROGRESS.with(|limit| limit.set(Some(1)));
    let context = crate::OperatorContext {
        evidence_store: Some(fixture.options.clone()),
        ..Default::default()
    };
    let result = crate::execute_with_context(
        Request::new("evidence.recover", json!({})),
        &crate::RunControl::default(),
        &context,
    );
    assert_eq!(result.execution.status, crate::result::Status::Cancelled);
    assert_eq!(
        result.data["completed"],
        json!({"recovered":[], "deleted":[ids[0]]})
    );
    assert!(!fixture.run(&ids[0]).exists());
    assert!(fixture.run(&ids[1]).exists());
}

#[test]
fn public_delete_of_unreferenced_artifacts_preserves_manifest_pins() {
    let fixture = Fixture::new();
    let mut store = fixture.open(true).unwrap();
    let mut manifest: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/sre-workbench/examples/evidence-interrupted-manifest.json"
    )))
    .unwrap();
    let id = crate::result::new_id("run");
    manifest["payload"]["run_id"] = json!(id);
    manifest["payload"]["finalized_at"] = json!("2026-10-10T12:00:01Z");
    let payload = manifest["payload"].clone();
    let reference = store
        .begin_collection(&id, payload.clone(), 128 * 1024)
        .unwrap()
        .seal(payload)
        .unwrap();
    crate::evidence::validate_bundle(&store.load_verified(&id).unwrap()).unwrap();
    let objects = store
        .runs
        .open_at(&id, true)
        .unwrap()
        .open_at("artifacts", true)
        .unwrap();
    let extra = receipt(json!({"extra":true}));
    objects
        .create_file(&format!("{}.json", sha256(&extra)))
        .unwrap()
        .write_all(&extra)
        .unwrap();
    drop(store);
    let context = crate::OperatorContext {
        evidence_store: Some(fixture.options.clone()),
        ..Default::default()
    };
    let wrong = crate::execute_with_context(
        Request::new(
            "evidence.delete",
            json!({"id":id, "expected_manifest_sha256":format!("sha256:{}", "0".repeat(64))}),
        ),
        &crate::RunControl::default(),
        &context,
    );
    assert_eq!(wrong.errors[0].code, "manifest_digest_mismatch");
    assert!(fixture.run(&id).exists());
    let deleted = crate::execute_with_context(
        Request::new(
            "evidence.delete",
            json!({"id":id, "expected_manifest_sha256":reference.digest}),
        ),
        &crate::RunControl::default(),
        &context,
    );
    assert_eq!(
        deleted.execution.status,
        crate::result::Status::Succeeded,
        "{deleted:?}"
    );
    assert_eq!(deleted.data["bundle"]["digest"], reference.digest);
    assert!(!fixture.run(&id).exists());
}

#[test]
fn sealed_receipts_survive_restart_with_private_run_relative_references() {
    let fixture = Fixture::new();
    let bytes = receipt(json!({"sample":17}));
    let mut store = fixture.open(true).unwrap();
    let (id, reference, artifact) = sealed(&mut store, &bytes);
    assert!(artifact.path.starts_with("artifacts/"));
    assert_eq!(
        stdfs::read(fixture.run(&id).join(&artifact.path)).unwrap(),
        bytes
    );
    let manifest_bytes = stdfs::read(fixture.run(&id).join("manifest.json")).unwrap();
    assert_eq!(
        reference.digest,
        format!("sha256:{}", sha256(&manifest_bytes))
    );
    for path in [
        fixture.options.directory.clone(),
        fixture.options.directory.join("runs"),
        fixture.run(&id),
        fixture.run(&id).join("artifacts"),
    ] {
        assert_eq!(stdfs::metadata(path).unwrap().mode() & 0o7777, 0o700);
    }
    for path in [
        fixture.options.directory.join("format.json"),
        fixture.options.directory.join("lock"),
        fixture.run(&id).join("intent.json"),
        fixture.run(&id).join("checkpoint.json"),
        fixture.run(&id).join("manifest.json"),
        fixture.run(&id).join(&artifact.path),
    ] {
        let metadata = stdfs::metadata(path).unwrap();
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        assert_eq!(metadata.nlink(), 1);
    }
    drop(store);
    let store = fixture.open(false).unwrap();
    let bundle = store.load_verified(&id).unwrap();
    assert_eq!(bundle.reference, reference);
    assert_eq!(bundle.artifacts["metrics"], bytes);
    assert_eq!(bundle.manifest.state, "sealed");
    assert!(!bundle.protected);
    assert_eq!(store.list().unwrap()[0].state, "sealed");
}

#[test]
fn locking_is_nonblocking_and_reads_never_initialize_a_store() {
    let fixture = Fixture::new();
    assert_eq!(code(fixture.open(false)), "store_not_found");
    assert!(!fixture.options.directory.exists());
    let first = fixture.open(true).unwrap();
    let start = Instant::now();
    assert_eq!(code(fixture.open(false)), "store_busy");
    assert!(start.elapsed() < Duration::from_secs(1));
    drop(first);
    assert!(fixture.open(false).is_ok());
}

#[test]
fn generated_ids_and_bound_policies_are_checked_before_writes() {
    let mut fixture = Fixture::new();
    fixture.options.max_bytes = 0;
    assert_eq!(code(fixture.open(true)), "store_invalid_options");
    assert!(!fixture.options.directory.exists());
    fixture.options.max_bytes = super::super::MAX_STORE_BYTES;
    fixture.options.retention_days = 0;
    assert_eq!(code(fixture.open(true)), "store_invalid_options");
    fixture.options.retention_days = 7;
    let mut store = fixture.open(true).unwrap();
    for id in [
        "../outside",
        "run-a-b-c",
        "run-1-AA-0",
        "run-1-2-0/extra",
        "run-1-2-0-0",
    ] {
        assert_eq!(
            code(store.begin_collection(id, json!({}), 2048)),
            "store_invalid_id"
        );
    }
    assert!(store.list().unwrap().is_empty());
    assert_eq!(
        code(store.begin_collection("run-1-2-0", json!({}), MAX_COLLECTION_BYTES + 1)),
        "store_invalid_reservation"
    );
    assert_eq!(
        code(store.begin_collection("run-1-2-0", json!({}), 1)),
        "store_reservation_exceeded"
    );
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn store_and_ancestor_symlinks_and_unsafe_modes_are_refused() {
    let fixture = Fixture::new();
    let outside = fixture.base.join("outside");
    stdfs::create_dir(&outside).unwrap();
    stdfs::set_permissions(&outside, stdfs::Permissions::from_mode(0o700)).unwrap();
    symlink(&outside, &fixture.options.directory).unwrap();
    assert_eq!(code(fixture.open(true)), "store_unsafe_entry");
    assert!(stdfs::read_dir(&outside).unwrap().next().is_none());
    stdfs::remove_file(&fixture.options.directory).unwrap();
    let store = fixture.open(true).unwrap();
    drop(store);
    stdfs::set_permissions(
        &fixture.options.directory,
        stdfs::Permissions::from_mode(0o750),
    )
    .unwrap();
    assert_eq!(code(fixture.open(false)), "store_unsafe_entry");
    stdfs::set_permissions(
        &fixture.options.directory,
        stdfs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let link = fixture.base.join("linked-parent");
    symlink(&fixture.options.directory, &link).unwrap();
    let mut options = fixture.options.clone();
    options.directory = link.join("child");
    assert_eq!(
        code(Store::open(
            &options,
            true,
            Instant::now() + Duration::from_secs(1)
        )),
        "store_unsafe_entry"
    );
}

#[test]
fn artifact_symlink_hardlink_fifo_and_unsafe_mode_are_refused() {
    for kind in ["symlink", "hardlink", "fifo", "mode"] {
        let fixture = Fixture::new();
        let mut store = fixture.open(true).unwrap();
        let (id, _, artifact) = sealed(&mut store, &receipt(json!({})));
        drop(store);
        let path = fixture.run(&id).join(artifact.path);
        let outside = fixture.base.join("outside");
        if kind == "hardlink" {
            stdfs::hard_link(&path, &outside).unwrap();
        } else if kind == "mode" {
            stdfs::set_permissions(&path, stdfs::Permissions::from_mode(0o640)).unwrap();
        } else {
            stdfs::rename(&path, &outside).unwrap();
            if kind == "symlink" {
                symlink(&outside, &path).unwrap();
            } else {
                use std::os::unix::ffi::OsStrExt;
                let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
                // SAFETY: the disposable fixture pathname is NUL-terminated and live.
                assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            }
        }
        let start = Instant::now();
        assert_eq!(code(fixture.open(false)), "store_unsafe_entry", "{kind}");
        assert!(start.elapsed() < Duration::from_secs(1));
        if kind != "mode" {
            assert!(!stdfs::read(&outside).unwrap().is_empty());
        }
    }
}

#[test]
fn duplicate_bytes_are_independent_across_runs_and_count_toward_quota() {
    let fixture = Fixture::new();
    let bytes = receipt(json!({"repeated":true}));
    let mut store = fixture.open(true).unwrap();
    let (first, _, first_ref) = sealed(&mut store, &bytes);
    let (second, _, second_ref) = sealed(&mut store, &bytes);
    assert_eq!(first_ref, second_ref);
    let left = stdfs::metadata(fixture.run(&first).join(&first_ref.path)).unwrap();
    let right = stdfs::metadata(fixture.run(&second).join(&second_ref.path)).unwrap();
    assert_ne!(left.ino(), right.ino());
    assert_eq!(left.nlink(), 1);
    assert_eq!(right.nlink(), 1);
    let all_bytes: u64 = store.list().unwrap().iter().map(|run| run.bytes).sum();
    assert!(all_bytes >= (bytes.len() * 2) as u64);
    let total = store.used_bytes;
    drop(store);
    let mut options = fixture.options.clone();
    options.max_bytes = total + 4095;
    let mut store = Store::open(&options, false, Instant::now() + Duration::from_secs(10)).unwrap();
    let id = crate::result::new_id("run");
    assert_eq!(
        code(store.begin_collection(&id, json!({}), 4096)),
        "store_quota_exceeded"
    );
    assert!(!fixture.run(&id).exists());
    assert_eq!(store.list().unwrap().len(), 2);
}

#[test]
fn a_refused_receipt_reservation_can_still_seal_an_honest_partial_bundle() {
    let fixture = Fixture::new();
    let mut store = fixture.open(true).unwrap();
    let id = crate::result::new_id("run");
    let mut lease = store.begin_collection(&id, json!({}), 2048).unwrap();
    let error = lease
        .put_receipt("metrics", &receipt(json!({"large":"x".repeat(4096)})))
        .unwrap_err();
    assert_eq!(error.code, "store_reservation_exceeded");
    assert!(!error.effects);
    assert!(lease.artifacts().is_empty());
    lease
        .seal(json!({"complete":false,"error":"reservation"}))
        .unwrap();
    drop(lease);
    assert!(store.load_verified(&id).unwrap().artifacts.is_empty());
}

#[test]
fn unsupported_formats_are_never_rewritten_or_removed() {
    let fixture = Fixture::new();
    let mut store = fixture.open(true).unwrap();
    let (id, _, _) = sealed(&mut store, &receipt(json!({})));
    drop(store);
    let manifest = fixture.run(&id).join("manifest.json");
    edit_json(&manifest, |value| value["format_version"] = json!(17));
    let original = stdfs::read(&manifest).unwrap();
    let mut store = fixture.open(false).unwrap();
    assert_eq!(code(store.load_verified(&id)), "store_unsupported_format");
    assert_eq!(store.list().unwrap()[0].state, "corrupt");
    assert_eq!(code(store.protect(&id, false)), "store_unsupported_format");
    assert_eq!(code(store.delete(&id)), "store_unsupported_format");
    assert_eq!(code(store.prune(u64::MAX)), "store_unsupported_format");
    assert_eq!(code(store.recover()), "store_unsupported_format");
    let next = crate::result::new_id("run");
    assert_eq!(
        code(store.begin_collection(&next, json!({}), 4096)),
        "store_unsupported_format"
    );
    assert_eq!(stdfs::read(&manifest).unwrap(), original);
    assert!(!fixture.run(&next).exists());
    drop(store);
    let format = fixture.options.directory.join("format.json");
    edit_json(&format, |value| value["format_version"] = json!(88));
    let original = stdfs::read(&format).unwrap();
    assert_eq!(code(fixture.open(true)), "store_unsupported_format");
    assert_eq!(stdfs::read(format).unwrap(), original);
}

#[test]
fn unknown_format_with_no_lock_is_untouched_and_initialized_locks_are_not_recreated() {
    let fixture = Fixture::new();
    stdfs::create_dir(&fixture.options.directory).unwrap();
    stdfs::set_permissions(
        &fixture.options.directory,
        stdfs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let format = fixture.options.directory.join("format.json");
    let unknown = b"{\"format_version\":937}\n";
    stdfs::write(&format, unknown).unwrap();
    stdfs::set_permissions(&format, stdfs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(code(fixture.open(true)), "store_unsupported_format");
    assert_eq!(stdfs::read(&format).unwrap(), unknown);
    assert_eq!(
        stdfs::read_dir(&fixture.options.directory).unwrap().count(),
        1
    );
    stdfs::write(&format, b"{\"format_version\":1}\n").unwrap();
    assert_eq!(code(fixture.open(true)), "store_uninitialized");
    assert_eq!(
        stdfs::read_dir(&fixture.options.directory).unwrap().count(),
        1
    );
}

#[test]
fn altered_receipt_bytes_and_cross_run_paths_are_detected_on_restart() {
    for corrupt_path in [false, true] {
        let fixture = Fixture::new();
        let mut store = fixture.open(true).unwrap();
        let (id, _, artifact) = sealed(&mut store, &receipt(json!({"sample":42})));
        drop(store);
        if corrupt_path {
            for file in ["manifest.json", "checkpoint.json"] {
                edit_json(&fixture.run(&id).join(file), |value| {
                    value["artifacts"]["metrics"]["path"] =
                        json!("../other-run/artifacts/forbidden.json");
                });
            }
        } else {
            let path = fixture.run(&id).join(artifact.path);
            let mut bytes = stdfs::read(&path).unwrap();
            let offset = bytes.windows(2).position(|part| part == b"42").unwrap();
            bytes[offset] = b'9';
            stdfs::write(path, bytes).unwrap();
        }
        let store = fixture.open(false).unwrap();
        assert_eq!(code(store.load_verified(&id)), "store_corrupt");
        assert_eq!(store.list().unwrap()[0].state, "corrupt");
    }
}

#[test]
fn malformed_duplicate_and_oversized_metadata_are_refused() {
    for malformed in [
        b"{broken".to_vec(),
        b"{\"format_version\":1,\"format_version\":1}".to_vec(),
        vec![b' '; MAX_METADATA_BYTES + 1],
    ] {
        let fixture = Fixture::new();
        let mut store = fixture.open(true).unwrap();
        let (id, _, _) = sealed(&mut store, &receipt(json!({})));
        drop(store);
        stdfs::write(fixture.run(&id).join("manifest.json"), &malformed).unwrap();
        if malformed.len() > MAX_METADATA_BYTES {
            assert_eq!(code(fixture.open(false)), "store_object_too_large");
        } else {
            let mut store = fixture.open(false).unwrap();
            assert_eq!(code(store.load_verified(&id)), "store_corrupt");
            assert_eq!(code(store.delete(&id)), "store_corrupt");
        }
        assert_eq!(
            stdfs::read(fixture.run(&id).join("manifest.json")).unwrap(),
            malformed
        );
    }
}

#[test]
fn recovery_marks_interrupted_without_replaying_or_claiming_checkpoint_completion() {
    let fixture = Fixture::new();
    let bytes = receipt(json!({"sample":13}));
    let id = crate::result::new_id("run");
    let mut store = fixture.open(true).unwrap();
    let mut lease = store
        .begin_collection(&id, json!({"complete":false}), 128 * 1024)
        .unwrap();
    let artifact = lease.put_receipt("metrics", &bytes).unwrap();
    lease
        .checkpoint(json!({"complete":true,"original_observation":"unchanged"}))
        .unwrap();
    drop(lease);
    drop(store);
    let mut store = fixture.open(false).unwrap();
    assert_eq!(store.list().unwrap()[0].state, "incomplete");
    assert_eq!(code(store.load_verified(&id)), "store_incomplete");
    assert_eq!(store.recover().unwrap().len(), 1);
    let recovered = store.load_verified(&id).unwrap();
    assert_eq!(recovered.manifest.state, "interrupted");
    assert_eq!(recovered.manifest.payload["complete"], true);
    assert_eq!(
        recovered.manifest.payload["original_observation"],
        "unchanged"
    );
    assert_eq!(recovered.manifest.artifacts["metrics"], artifact);
    assert_eq!(recovered.artifacts["metrics"], bytes);
    assert!(store.recover().unwrap().is_empty());
    assert!(store.prune(u64::MAX).unwrap().is_empty());
}

#[test]
fn injected_write_sync_and_publish_failures_preserve_staging_and_withhold_references() {
    // This exercises failures at syscall boundaries; it is not a real disk-full proof.
    for point in [
        FailurePoint::Write,
        FailurePoint::FileSync,
        FailurePoint::Publish,
        FailurePoint::DirectorySync,
    ] {
        let fixture = Fixture::new();
        let id = crate::result::new_id("run");
        let bytes = receipt(json!({"point":format!("{point:?}")}));
        let mut store = fixture.open(true).unwrap();
        let mut lease = store.begin_collection(&id, json!({}), 128 * 1024).unwrap();
        FAILURE.with(|failure| failure.set(Some((point, 0))));
        let error = lease.put_receipt("metrics", &bytes).unwrap_err();
        assert!(error.effects);
        assert!(lease.artifacts().is_empty());
        assert!(!fixture.run(&id).join("manifest.json").exists());
        let retained: Vec<_> = stdfs::read_dir(fixture.run(&id).join("artifacts"))
            .unwrap()
            .collect();
        assert_eq!(retained.len(), 1, "{point:?}");
        if point == FailurePoint::Write {
            let retained = retained[0].as_ref().unwrap().metadata().unwrap().len();
            assert!(retained > 0 && retained < bytes.len() as u64);
        }
        drop(lease);
        drop(store);
        let mut store = fixture.open(false).unwrap();
        let recovered = store.recover().unwrap();
        assert_eq!(recovered.len(), 1);
        let bundle = store.load_verified(&id).unwrap();
        assert_eq!(bundle.manifest.state, "interrupted");
        assert_eq!(
            bundle.artifacts.len(),
            usize::from(point == FailurePoint::DirectorySync)
        );
        if point == FailurePoint::DirectorySync {
            assert!(
                bundle
                    .artifacts
                    .keys()
                    .next()
                    .unwrap()
                    .starts_with("recovered-")
            );
            assert_eq!(bundle.artifacts.values().next().unwrap(), &bytes);
        }
    }
}

#[test]
fn checkpoint_failure_exposes_the_confirmed_object_and_recovery_preserves_it() {
    let fixture = Fixture::new();
    let id = crate::result::new_id("run");
    let bytes = receipt(json!({"retained":true}));
    let mut store = fixture.open(true).unwrap();
    let mut lease = store.begin_collection(&id, json!({}), 128 * 1024).unwrap();
    FAILURE.with(|failure| failure.set(Some((FailurePoint::Publish, 1))));
    assert_eq!(
        lease.put_receipt("metrics", &bytes).unwrap_err().code,
        "store_injected_failure"
    );
    let retained = lease.artifacts().get("metrics").unwrap().clone();
    assert_eq!(
        stdfs::read(fixture.run(&id).join(&retained.path)).unwrap(),
        bytes
    );
    assert_eq!(
        code(lease.seal(json!({"complete":false}))),
        "store_unresolved"
    );
    drop(lease);
    drop(store);
    let mut store = fixture.open(false).unwrap();
    store.recover().unwrap();
    let bundle = store.load_verified(&id).unwrap();
    assert_eq!(bundle.manifest.state, "interrupted");
    assert_eq!(
        bundle.manifest.artifacts.values().next().unwrap(),
        &retained
    );
    assert_eq!(bundle.artifacts.values().next().unwrap(), &bytes);
}

#[test]
fn seal_failure_never_returns_a_bundle_reference() {
    for point in [
        FailurePoint::Write,
        FailurePoint::FileSync,
        FailurePoint::Publish,
        FailurePoint::DirectorySync,
    ] {
        let fixture = Fixture::new();
        let id = crate::result::new_id("run");
        let mut store = fixture.open(true).unwrap();
        let mut lease = store.begin_collection(&id, json!({}), 128 * 1024).unwrap();
        lease.put_receipt("metrics", &receipt(json!({}))).unwrap();
        FAILURE.with(|failure| failure.set(Some((point, 1))));
        let result = lease.seal(json!({"complete":true}));
        assert_eq!(code(result), "store_injected_failure");
        assert_eq!(lease.artifacts().len(), 1);
        drop(lease);
        drop(store);
        let mut store = fixture.open(false).unwrap();
        if point == FailurePoint::DirectorySync {
            // The name became visible but the failed fsync did not establish a durable commit
            // for the original caller. A subsequent verified read may observe surviving bytes.
            assert!(store.recover().unwrap().is_empty());
        } else {
            assert_eq!(store.recover().unwrap().len(), 1);
            assert_eq!(
                store.load_verified(&id).unwrap().manifest.state,
                "interrupted"
            );
        }
    }
}

#[test]
fn malformed_canonical_orphan_receipt_is_preserved_and_refused_by_recovery() {
    let fixture = Fixture::new();
    let id = crate::result::new_id("run");
    let mut store = fixture.open(true).unwrap();
    drop(store.begin_collection(&id, json!({}), 128 * 1024).unwrap());
    drop(store);
    let invalid = b"{\"unexpected\":true}\n";
    let path = fixture
        .run(&id)
        .join("artifacts")
        .join(format!("{}.json", sha256(invalid)));
    stdfs::write(&path, invalid).unwrap();
    stdfs::set_permissions(&path, stdfs::Permissions::from_mode(0o600)).unwrap();
    let mut store = fixture.open(false).unwrap();
    assert_eq!(code(store.recover()), "store_unsupported_receipt");
    assert!(!fixture.run(&id).join("manifest.json").exists());
    assert_eq!(stdfs::read(path).unwrap(), invalid);
}

#[test]
fn protection_is_mutable_without_changing_manifest_identity_and_blocks_removal() {
    let fixture = Fixture::new();
    let mut store = fixture.open(true).unwrap();
    let (id, reference, _) = sealed(&mut store, &receipt(json!({})));
    store.protect(&id, true).unwrap();
    let protected = store.load_verified(&id).unwrap();
    assert!(protected.protected);
    assert_eq!(protected.reference, reference);
    assert_eq!(code(store.delete(&id)), "store_protected");
    assert!(store.prune(u64::MAX).unwrap().is_empty());
    store.protect(&id, false).unwrap();
    assert_eq!(store.load_verified(&id).unwrap().reference, reference);
    store.delete(&id).unwrap();
    assert!(!fixture.run(&id).exists());
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn retention_only_removes_old_unprotected_resolved_sealed_runs() {
    let fixture = Fixture::new();
    let mut store = fixture.open(true).unwrap();
    let (old, _, _) = sealed(&mut store, &receipt(json!({"kind":"old"})));
    let (protected, _, _) = sealed(&mut store, &receipt(json!({"kind":"protected"})));
    store.protect(&protected, true).unwrap();
    let (unresolved, _, _) = sealed(&mut store, &receipt(json!({"kind":"unresolved"})));
    let temporary = fixture.run(&unresolved).join(".tmp-1-2-3");
    stdfs::write(&temporary, b"partial").unwrap();
    stdfs::set_permissions(temporary, stdfs::Permissions::from_mode(0o600)).unwrap();
    let (young, _, _) = sealed(&mut store, &receipt(json!({"kind":"young"})));
    let now = now_unix_ms() + 8 * 86_400_000;
    edit_json(&fixture.run(&young).join("manifest.json"), |value| {
        value["finalized_at_unix_ms"] = json!(now)
    });
    let interrupted = crate::result::new_id("run");
    drop(
        store
            .begin_collection(&interrupted, json!({}), 128 * 1024)
            .unwrap(),
    );
    store.recover().unwrap();
    let incomplete = crate::result::new_id("run");
    drop(
        store
            .begin_collection(&incomplete, json!({}), 128 * 1024)
            .unwrap(),
    );
    assert_eq!(store.prune(now).unwrap(), vec![old]);
    for id in [protected, unresolved, young, interrupted, incomplete] {
        assert!(fixture.run(&id).exists());
    }
}

#[test]
fn partial_deletion_retries_use_the_original_manifest_digest() {
    let fixture = Fixture::new();
    let mut store = fixture.open(true).unwrap();
    let (id, reference, artifact) = sealed(&mut store, &receipt(json!({})));
    drop(store);
    let run = fixture.run(&id);
    stdfs::rename(run.join("manifest.json"), run.join("deletion.json")).unwrap();
    stdfs::remove_file(run.join(artifact.path)).unwrap();
    let mut store = fixture.open(false).unwrap();
    assert_eq!(code(store.load_verified(&id)), "store_deleting");
    assert_eq!(store.deletion_reference(&id).unwrap(), reference);
    store.delete(&id).unwrap();
    assert!(!run.exists());
}

#[test]
fn root_deletion_journal_preserves_digest_before_and_after_run_directory_removal() {
    for point in [
        FailurePoint::BeforeRunRemoval,
        FailurePoint::AfterRunRemoval,
    ] {
        let fixture = Fixture::new();
        let mut store = fixture.open(true).unwrap();
        let (id, reference, _) = sealed(&mut store, &receipt(json!({})));
        FAILURE.with(|failure| failure.set(Some((point, 0))));
        assert_eq!(code(store.delete(&id)), "store_injected_failure");
        let journal = fixture.options.directory.join(journal_name(&id));
        assert!(journal.exists());
        assert_eq!(
            fixture.run(&id).exists(),
            point == FailurePoint::BeforeRunRemoval
        );
        drop(store);
        let mut store = fixture.open(false).unwrap();
        assert_eq!(code(store.load_verified(&id)), "store_deleting");
        assert_eq!(store.deletion_reference(&id).unwrap(), reference);
        assert_eq!(store.list().unwrap()[0].error, Some("store_deleting"));
        assert_eq!(code(store.recover()), "store_deleting");
        store.delete(&id).unwrap();
        assert!(!fixture.run(&id).exists());
        assert!(!journal.exists());
        assert!(store.list().unwrap().is_empty());
    }
}

#[test]
fn receipt_nested_shapes_and_bounds_are_checked_before_publication() {
    let mut corruptions: Vec<(Vec<&str>, Value)> = vec![
        (vec!["execution", "unexpected"], json!(true)),
        (vec!["execution", "child_exit_code"], json!("0")),
        (vec!["execution", "signal"], json!(17)),
        (vec!["coverage", "state"], json!("unknown")),
        (vec!["coverage", "limitations"], json!([""])),
        (vec!["output", "unexpected"], json!(false)),
        (vec!["output", "encoding"], json!("binary")),
        (vec!["effective_limits", "timeout_ms"], json!(0)),
        (
            vec!["effective_limits", "max_output_bytes"],
            json!(MAX_RESULT_BYTES + 1),
        ),
        (vec!["effective_limits", "max_items"], Value::Null),
        (vec!["effective_limits", "concurrency"], json!(17)),
        (
            vec!["errors"],
            json!([{"code":"bad","message":"bad","retryable":false,"unexpected":1}]),
        ),
        (
            vec!["errors"],
            json!([{"code":"Bad","message":"bad","retryable":false}]),
        ),
        (
            vec!["sources"],
            json!([{"reference":"x","observed_at":null,"evidence_state":"invented","untrusted":true}]),
        ),
    ];
    let fixture = Fixture::new();
    let mut store = fixture.open(true).unwrap();
    let id = crate::result::new_id("run");
    let mut lease = store.begin_collection(&id, json!({}), 128 * 1024).unwrap();
    for (path, replacement) in corruptions.drain(..) {
        let mut value: Value = serde_json::from_slice(&receipt(json!({}))).unwrap();
        let mut destination = &mut value;
        for part in path {
            destination = &mut destination[part];
        }
        *destination = replacement;
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        assert_eq!(
            code(lease.put_receipt("metrics", &bytes)),
            "store_invalid_receipt"
        );
    }
    assert!(lease.artifacts().is_empty());
    assert_eq!(
        stdfs::read_dir(fixture.run(&id).join("artifacts"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn deadline_refusal_is_effect_free_and_finalization_allowance_is_explicit() {
    let fixture = Fixture::new();
    assert_eq!(
        code(Store::open(&fixture.options, true, Instant::now())),
        "store_deadline_exceeded"
    );
    assert!(!fixture.options.directory.exists());
    let mut store = fixture.open(true).unwrap();
    let id = crate::result::new_id("run");
    let mut lease = store.begin_collection(&id, json!({}), 128 * 1024).unwrap();
    lease.set_deadline(Instant::now());
    let error = lease
        .put_receipt("metrics", &receipt(json!({})))
        .unwrap_err();
    assert_eq!(error.code, "store_deadline_exceeded");
    assert!(!error.effects);
    lease.set_deadline(Instant::now() + Duration::from_millis(2500));
    lease
        .seal(json!({"complete":false,"cancelled":true}))
        .unwrap();
}

fn fixture_validator(bundle: &VerifiedBundle) -> Result<(), StoreError> {
    if bundle.manifest.payload["version"] != 1 {
        return Err(StoreError::new("store_unsupported_payload", false));
    }
    if bundle.manifest.payload["coherent"] != true {
        return Err(StoreError::new("store_corrupt", false));
    }
    Ok(())
}

fn open_managed(fixture: &Fixture, signal: Arc<AtomicUsize>) -> Store {
    Store::open_validated(
        &fixture.options,
        false,
        Instant::now() + Duration::from_secs(30),
        fixture_validator,
        Some(signal),
    )
    .unwrap()
}

fn domain_run(store: &mut Store, sealed: bool, version: u64, coherent: bool) -> String {
    let id = crate::result::new_id("run");
    let payload = json!({"version":version,"coherent":coherent});
    let mut lease = store
        .begin_collection(&id, payload.clone(), 128 * 1024)
        .unwrap();
    lease
        .put_receipt("metrics", &receipt(json!({"sample":17})))
        .unwrap();
    if sealed {
        lease.seal(payload).unwrap();
    }
    id
}

fn snapshot_tree(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(path: &Path, output: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in stdfs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(&entry.path(), output);
            } else {
                output.insert(entry.path(), stdfs::read(entry.path()).unwrap());
            }
        }
    }
    let mut output = BTreeMap::new();
    visit(path, &mut output);
    output
}

#[test]
fn domain_hook_classifies_and_preflights_all_runs_before_prune_or_recovery() {
    for (version, coherent, expected) in [
        (2, true, "store_unsupported_payload"),
        (1, false, "store_corrupt"),
    ] {
        for terminal in [true, false] {
            let fixture = Fixture::new();
            let mut store = fixture.open(true).unwrap();
            let valid = domain_run(&mut store, terminal, 1, true);
            let invalid = domain_run(&mut store, terminal, version, coherent);
            drop(store);
            let before = snapshot_tree(&fixture.options.directory);
            let mut store = open_managed(&fixture, Arc::new(AtomicUsize::new(0)));
            let summaries = store.list().unwrap();
            let summary = summaries
                .iter()
                .find(|summary| summary.id == invalid)
                .unwrap();
            assert_eq!(summary.state, "corrupt");
            assert_eq!(summary.error, Some(expected));
            assert_eq!(code(store.prune(u64::MAX)), expected);
            assert_eq!(code(store.recover()), expected);
            assert_eq!(code(store.delete(&invalid)), expected);
            assert!(store.progress().deleted.is_empty());
            assert!(store.progress().recovered.is_empty());
            assert_eq!(snapshot_tree(&fixture.options.directory), before);
            assert!(fixture.run(&valid).exists());
        }
    }
}

#[test]
fn domain_hook_checks_deletion_journal_payload_without_requiring_removed_receipts() {
    let fixture = Fixture::new();
    let mut store = fixture.open(true).unwrap();
    let id = domain_run(&mut store, true, 1, true);
    let original = store.load_verified(&id).unwrap().reference;
    FAILURE.with(|failure| failure.set(Some((FailurePoint::AfterRunRemoval, 0))));
    assert_eq!(code(store.delete(&id)), "store_injected_failure");
    drop(store);
    let journal = fixture.options.directory.join(journal_name(&id));
    let supported = stdfs::read(&journal).unwrap();
    edit_json(&journal, |value| value["payload"]["version"] = json!(2));
    let unsupported = snapshot_tree(&fixture.options.directory);
    let mut store = open_managed(&fixture, Arc::new(AtomicUsize::new(0)));
    assert_eq!(
        code(store.deletion_reference(&id)),
        "store_unsupported_payload"
    );
    assert_eq!(code(store.delete(&id)), "store_unsupported_payload");
    assert_eq!(snapshot_tree(&fixture.options.directory), unsupported);
    drop(store);
    stdfs::write(&journal, supported).unwrap();
    let mut store = open_managed(&fixture, Arc::new(AtomicUsize::new(0)));
    assert_eq!(store.deletion_reference(&id).unwrap(), original);
    store.delete(&id).unwrap();
    assert_eq!(store.progress().deleted, vec![id]);
}

#[test]
fn cancellation_before_management_access_or_mutation_preserves_every_byte() {
    let fixture = Fixture::new();
    let mut store = fixture.open(true).unwrap();
    domain_run(&mut store, true, 1, true);
    drop(store);
    let before = snapshot_tree(&fixture.options.directory);
    let signal = Arc::new(AtomicUsize::new(2));
    let error = match Store::open_validated(
        &fixture.options,
        false,
        Instant::now() + Duration::from_secs(30),
        fixture_validator,
        Some(signal.clone()),
    ) {
        Err(error) => error,
        Ok(_) => panic!("cancelled open must refuse access"),
    };
    assert_eq!(error.code, "store_cancelled");
    assert!(!error.effects);
    signal.store(0, Ordering::Relaxed);
    let mut store = open_managed(&fixture, signal.clone());
    signal.store(15, Ordering::Relaxed);
    let error = store.prune(u64::MAX).unwrap_err();
    assert_eq!(error.code, "store_cancelled");
    assert!(!error.effects);
    assert!(store.progress().deleted.is_empty());
    assert_eq!(snapshot_tree(&fixture.options.directory), before);
}

#[test]
fn cancellation_after_completed_bulk_action_retains_exact_progress_and_stops_next_run() {
    for recovery in [false, true] {
        let fixture = Fixture::new();
        let mut store = fixture.open(true).unwrap();
        let first = domain_run(&mut store, !recovery, 1, true);
        let second = domain_run(&mut store, !recovery, 1, true);
        drop(store);
        let mut ids = [first, second];
        ids.sort();
        let untouched = snapshot_tree(&fixture.run(&ids[1]));
        let mut store = open_managed(&fixture, Arc::new(AtomicUsize::new(0)));
        CANCEL_AFTER_PROGRESS.with(|limit| limit.set(Some(1)));
        let error = if recovery {
            store.recover().unwrap_err()
        } else {
            store.prune(u64::MAX).unwrap_err()
        };
        assert_eq!(error.code, "store_cancelled");
        assert!(error.effects);
        if recovery {
            assert_eq!(store.progress().recovered.len(), 1);
            assert_eq!(store.progress().recovered[0].id, ids[0]);
            assert!(fixture.run(&ids[0]).join("manifest.json").exists());
        } else {
            assert_eq!(store.progress().deleted, vec![ids[0].clone()]);
            assert!(!fixture.run(&ids[0]).exists());
        }
        assert_eq!(snapshot_tree(&fixture.run(&ids[1])), untouched);
    }
}

#[test]
fn shared_receipt_depth_covers_response_and_envelope_without_unbounded_json() {
    let mut data = json!({"leaf":"retained"});
    for _ in 0..60 {
        data = json!({"nested":data});
    }
    assert!(validate_receipt(&receipt(data)).is_ok());
    let mut data = json!({"leaf":"too deep"});
    for _ in 0..crate::result::MAX_RECEIPT_DEPTH {
        data = json!({"nested":data});
    }
    assert_eq!(
        code(validate_receipt(&receipt(data))),
        "store_invalid_receipt"
    );
}
