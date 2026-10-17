//! Private, bounded runbook journals. No implicit repair, migration, or staging cleanup.
use crate::evidence::store::StoreError;
use serde_json::Value;
use std::{path::Path, time::Instant};

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use crate::{
        evidence::store::{
            fs::{self, Directory, io_error},
            valid_run_id,
        },
        request::parse_bounded_json,
    };
    use sha2::{Digest, Sha256};
    use std::{
        fs::File,
        io::Write,
        sync::atomic::{AtomicU64, Ordering},
    };

    const MAX_RUNS: usize = 64;
    const MAX_STAGING: usize = 16;
    const MAX_STATE: usize = 4 * 1024 * 1024;
    const MAX_TOTAL: u64 = 128 * 1024 * 1024;
    const FORMAT: &[u8] = b"{\"format_version\":1,\"kind\":\"guided-runbooks\"}\n";
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    #[derive(Debug)]
    pub(in crate::runbooks) struct Store {
        root: Directory,
        _lock: File,
        deadline: Instant,
        finalization_ceiling: Instant,
        effects: bool,
    }

    #[derive(Default)]
    struct Layout {
        bytes: u64,
        runs: usize,
        staging: usize,
        format: bool,
        lock: bool,
    }

    fn check(deadline: Instant) -> Result<(), StoreError> {
        if Instant::now() >= deadline {
            Err(StoreError::new("store_deadline_exceeded", false))
        } else {
            Ok(())
        }
    }

    fn state(bytes: &[u8], id: &str) -> Result<Value, StoreError> {
        let value =
            parse_bounded_json(bytes, 80).map_err(|_| StoreError::new("store_corrupt", false))?;
        if !value.is_object()
            || value.get("run_id").and_then(Value::as_str) != Some(id)
            || value
                .get("revision")
                .and_then(Value::as_u64)
                .is_none_or(|v| v == 0)
        {
            return Err(StoreError::new("store_corrupt", false));
        }
        if value.get("format_version").and_then(Value::as_u64) != Some(1) {
            return Err(StoreError::new("store_unsupported_format", false));
        }
        Ok(value)
    }

    fn run_name(name: &str) -> Option<&str> {
        name.strip_suffix(".json").filter(|id| valid_run_id(id))
    }

    fn scan(root: &Directory, deadline: Instant) -> Result<Layout, StoreError> {
        check(deadline)?;
        let names = root.names(MAX_RUNS + MAX_STAGING + 2)?;
        let mut layout = Layout::default();
        for name in names {
            check(deadline)?;
            let maximum = if name == "format.json" {
                layout.format = true;
                1024
            } else if name == "lock" {
                layout.lock = true;
                0
            } else if run_name(&name).is_some() {
                layout.runs += 1;
                MAX_STATE
            } else if name.strip_prefix(".tmp-").is_some_and(valid_run_id) {
                layout.staging += 1;
                MAX_STATE
            } else {
                return Err(StoreError::new("store_unsafe_entry", false));
            };
            let file = root.open_file(&name, false)?;
            check(deadline)?;
            let length = file.metadata().map_err(|e| io_error(e, false))?.len();
            if length > maximum as u64 {
                return Err(StoreError::new("store_object_too_large", false));
            }
            layout.bytes = layout
                .bytes
                .checked_add(length)
                .ok_or(StoreError::new("store_quota_exceeded", false))?;
            if name == "format.json" {
                let bytes = fs::read(root, &name, 1024)?;
                let value = parse_bounded_json(&bytes, 4)
                    .map_err(|_| StoreError::new("store_corrupt", false))?;
                if value != serde_json::json!({"format_version":1,"kind":"guided-runbooks"}) {
                    return Err(StoreError::new("store_unsupported_format", false));
                }
            } else if let Some(id) = run_name(&name) {
                let bytes = fs::read(root, &name, MAX_STATE)?;
                check(deadline)?;
                state(&bytes, id)?;
                check(deadline)?;
            }
            if layout.runs > MAX_RUNS || layout.staging > MAX_STAGING {
                return Err(StoreError::new("store_layout_limit", false));
            }
            if layout.bytes > MAX_TOTAL {
                return Err(StoreError::new("store_quota_exceeded", false));
            }
        }
        check(deadline)?;
        Ok(layout)
    }

    fn digest(bytes: &[u8]) -> String {
        let hex = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("sha256:{hex}")
    }

    impl Store {
        pub(in crate::runbooks) fn open(
            directory: &Path,
            create: bool,
            deadline: Instant,
        ) -> Result<Self, StoreError> {
            check(deadline)?;
            if !directory.is_absolute() {
                return Err(StoreError::new("store_invalid_options", false));
            }
            let (root, created) = fs::open_root(directory, create)?;
            let result = (|| {
                let layout = scan(&root, deadline)?;
                // A partially initialized root is not silently repaired. In particular, never
                // create a lock in an evidence store or an unrecognized nonempty directory.
                let empty =
                    !layout.format && !layout.lock && layout.runs == 0 && layout.staging == 0;
                if !empty && (!layout.format || !layout.lock) {
                    return Err(StoreError::new("store_incomplete", false));
                }
                if empty && !create {
                    return Err(StoreError::new("store_not_found", false));
                }
                check(deadline)?;
                let (lock, lock_created) = fs::lock(&root, empty && create)?;
                let mut store = Self {
                    root,
                    _lock: lock,
                    deadline,
                    finalization_ceiling: deadline + std::time::Duration::from_millis(2500),
                    effects: created || lock_created,
                };
                let initialized = (|| {
                    check(deadline)?;
                    // Recheck after acquiring the lock: a concurrent initializer may have won.
                    let locked_layout = scan(&store.root, deadline)?;
                    if !locked_layout.format {
                        if !empty || locked_layout.runs != 0 || locked_layout.staging != 0 {
                            return Err(StoreError::new("store_incomplete", false));
                        }
                        store.publish("format.json", FORMAT, false)?;
                    }
                    Ok(())
                })();
                initialized
                    .map_err(|e: StoreError| if store.effects { e.after_effects() } else { e })?;
                Ok(store)
            })();
            result.map_err(|e| if created { e.after_effects() } else { e })
        }

        /// Narrow active work and its associated finalization ceiling together.
        pub(in crate::runbooks) fn set_deadline(&mut self, deadline: Instant) {
            self.deadline = self.deadline.min(deadline);
            self.finalization_ceiling = self
                .finalization_ceiling
                .min(deadline + std::time::Duration::from_millis(2500));
        }

        /// Only persist already-observed work under the original bounded cleanup allowance.
        /// The caller must restore the active deadline before admitting more work.
        pub(in crate::runbooks) fn allow_finalization(&mut self, deadline: Instant) {
            self.finalization_ceiling = self.finalization_ceiling.min(deadline);
            self.deadline = self.finalization_ceiling;
        }

        pub(in crate::runbooks) fn load(&self, id: &str) -> Result<(Value, String), StoreError> {
            let result = (|| {
                check(self.deadline)?;
                if !valid_run_id(id) {
                    return Err(StoreError::new("store_invalid_reference", false));
                }
                self.layout()?;
                let bytes = fs::read(&self.root, &format!("{id}.json"), MAX_STATE)?;
                check(self.deadline)?;
                let value = state(&bytes, id)?;
                let digest = digest(&bytes);
                check(self.deadline)?;
                Ok((value, digest))
            })();
            result.map_err(|e| if self.effects { e.after_effects() } else { e })
        }

        pub(in crate::runbooks) fn create(
            &mut self,
            id: &str,
            value: &Value,
        ) -> Result<String, StoreError> {
            self.write(id, None, value)
        }

        pub(in crate::runbooks) fn replace(
            &mut self,
            id: &str,
            expected_revision: u64,
            value: &Value,
        ) -> Result<String, StoreError> {
            self.write(id, Some(expected_revision), value)
        }

        fn write(
            &mut self,
            id: &str,
            expected: Option<u64>,
            value: &Value,
        ) -> Result<String, StoreError> {
            let result = (|| {
                check(self.deadline)?;
                if !valid_run_id(id) {
                    return Err(StoreError::new("store_invalid_reference", false));
                }
                let mut bytes = serde_json::to_vec(value)
                    .map_err(|_| StoreError::new("store_corrupt", false))?;
                bytes.push(b'\n');
                if bytes.len() > MAX_STATE {
                    return Err(StoreError::new("store_object_too_large", false));
                }
                let validated = state(&bytes, id)?;
                let required_revision = match expected {
                    Some(revision) => {
                        let (old, _) = self.load(id)?;
                        if old.get("revision").and_then(Value::as_u64) != Some(revision) {
                            return Err(StoreError::new("store_revision_conflict", false));
                        }
                        revision
                            .checked_add(1)
                            .ok_or(StoreError::new("store_revision_conflict", false))?
                    }
                    None => 1,
                };
                if validated.get("revision").and_then(Value::as_u64) != Some(required_revision) {
                    return Err(StoreError::new("store_revision_conflict", false));
                }
                let name = format!("{id}.json");
                if expected.is_none() {
                    match self.root.open_file(&name, false) {
                        Ok(_) => return Err(StoreError::new("store_collision", false)),
                        Err(e) if e.code == "store_not_found" => {}
                        Err(e) => return Err(e),
                    }
                }
                let layout = self.layout()?;
                if expected.is_none() && layout.runs >= MAX_RUNS || layout.staging >= MAX_STAGING {
                    return Err(StoreError::new("store_layout_limit", false));
                }
                if layout.bytes.saturating_add(bytes.len() as u64) > MAX_TOTAL {
                    return Err(StoreError::new("store_quota_exceeded", false));
                }
                self.publish(&name, &bytes, expected.is_some())?;
                Ok(digest(&bytes))
            })();
            result.map_err(|e| if self.effects { e.after_effects() } else { e })
        }

        fn layout(&self) -> Result<Layout, StoreError> {
            let layout = scan(&self.root, self.deadline)?;
            if !layout.format || !layout.lock {
                return Err(StoreError::new("store_incomplete", false));
            }
            Ok(layout)
        }

        fn publish(&mut self, name: &str, bytes: &[u8], replace: bool) -> Result<(), StoreError> {
            check(self.deadline)?;
            let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            // A collision refuses rather than overwriting a retained interrupted write.
            let temporary = format!(
                ".tmp-run-{}-{:x}-{:x}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| StoreError::new("store_clock", false))?
                    .as_nanos(),
                nonce
            );
            let mut file = self.root.create_file(&temporary)?;
            self.effects = true;
            check(self.deadline).map_err(StoreError::after_effects)?;
            file.write_all(bytes).map_err(|e| io_error(e, true))?;
            check(self.deadline).map_err(StoreError::after_effects)?;
            file.sync_all().map_err(|e| io_error(e, true))?;
            check(self.deadline).map_err(StoreError::after_effects)?;
            self.root.publish(&temporary, name, replace)?;
            check(self.deadline).map_err(StoreError::after_effects)?;
            self.root.sync()?;
            check(self.deadline).map_err(StoreError::after_effects)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{
            fs,
            os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
            path::PathBuf,
            time::Duration,
        };
        static NEXT_TEST: AtomicU64 = AtomicU64::new(1);
        const ID: &str = "run-1-ab-cd";
        struct Root(PathBuf);
        impl Root {
            fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "guided-store-{}-{}",
                    std::process::id(),
                    NEXT_TEST.fetch_add(1, Ordering::Relaxed)
                ));
                fs::create_dir(&path).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
                Self(path)
            }
            fn open(&self, create: bool) -> Result<Store, StoreError> {
                Store::open(&self.0, create, Instant::now() + Duration::from_secs(30))
            }
            fn put(&self, name: &str, bytes: &[u8]) {
                let mut f = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(self.0.join(name))
                    .unwrap();
                f.write_all(bytes).unwrap();
            }
            fn names(&self) -> Vec<String> {
                let mut names = fs::read_dir(&self.0)
                    .unwrap()
                    .map(|e| e.unwrap().file_name().into_string().unwrap())
                    .collect::<Vec<_>>();
                names.sort();
                names
            }
        }
        impl Drop for Root {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
        fn value(revision: u64) -> Value {
            serde_json::json!({"format_version":1,"run_id":ID,"revision":revision,"state":"paused"})
        }

        #[test]
        fn restart_exact_bytes_digest_and_revision_cas() {
            let root = Root::new();
            let mut store = root.open(true).unwrap();
            let first = store.create(ID, &value(1)).unwrap();
            let bytes = fs::read(root.0.join(format!("{ID}.json"))).unwrap();
            assert!(bytes.ends_with(b"\n"));
            assert_eq!(
                first,
                "sha256:5e870ebd7a0eeb772fe41766306fb1b592740b1429bba5e1d6f41798d49d18af"
            );
            drop(store);
            let mut store = root.open(false).unwrap();
            assert_eq!(store.load(ID).unwrap(), (value(1), first));
            assert_eq!(
                store.create(ID, &value(1)).unwrap_err(),
                StoreError::new("store_collision", false)
            );
            assert_eq!(
                store.replace(ID, 2, &value(3)).unwrap_err(),
                StoreError::new("store_revision_conflict", false)
            );
            assert_eq!(
                store.replace(ID, 1, &value(3)).unwrap_err(),
                StoreError::new("store_revision_conflict", false)
            );
            store.replace(ID, 1, &value(2)).unwrap();
            drop(store);
            assert_eq!(root.open(false).unwrap().load(ID).unwrap().0, value(2));
        }

        #[test]
        fn unrelated_and_unsupported_roots_remain_untouched() {
            for (name, bytes, expected) in [
                (
                    "format.json",
                    &b"{\"format_version\":1}"[..],
                    "store_unsupported_format",
                ),
                (
                    "format.json",
                    &b"{\"format_version\":2,\"kind\":\"guided-runbooks\"}"[..],
                    "store_unsupported_format",
                ),
                ("other", &b"x"[..], "store_unsafe_entry"),
                ("lock", &b""[..], "store_incomplete"),
            ] {
                let root = Root::new();
                root.put(name, bytes);
                let before = root.names();
                assert_eq!(
                    root.open(true).unwrap_err(),
                    StoreError::new(expected, false)
                );
                assert_eq!(root.names(), before);
                assert_eq!(fs::read(root.0.join(name)).unwrap(), bytes);
            }
            let root = Root::new();
            assert_eq!(
                root.open(false).unwrap_err(),
                StoreError::new("store_not_found", false)
            );
            assert!(root.names().is_empty());
        }

        #[test]
        fn unsafe_files_and_directory_permissions_are_refused() {
            for mode in ["symlink", "hardlink", "permissions", "fifo"] {
                let root = Root::new();
                drop(root.open(true).unwrap());
                let path = root.0.join(format!("{ID}.json"));
                match mode {
                    "symlink" => symlink("format.json", &path).unwrap(),
                    "hardlink" => fs::hard_link(root.0.join("format.json"), &path).unwrap(),
                    "permissions" => {
                        root.put(&format!("{ID}.json"), b"{}");
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
                    }
                    "fifo" => {
                        use std::os::unix::ffi::OsStrExt;
                        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
                        // SAFETY: the CString supplies a valid NUL-terminated private fixture path.
                        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
                    }
                    _ => unreachable!(),
                }
                assert_eq!(root.open(false).unwrap_err().code, "store_unsafe_entry");
            }
            let root = Root::new();
            fs::set_permissions(&root.0, fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(root.open(true).unwrap_err().code, "store_unsafe_entry");
            assert!(root.names().is_empty());
        }

        #[test]
        fn lock_and_deadline_refuse_without_mutation() {
            let root = Root::new();
            let store = root.open(true).unwrap();
            assert_eq!(
                root.open(false).unwrap_err(),
                StoreError::new("store_busy", false)
            );
            assert_eq!(
                Store::open(&root.0, true, Instant::now()).unwrap_err(),
                StoreError::new("store_deadline_exceeded", false)
            );
            drop(store);
            let mut store = root.open(false).unwrap();
            let earlier = Instant::now();
            store.set_deadline(earlier);
            store.set_deadline(earlier + Duration::from_secs(30));
            assert_eq!(store.deadline, earlier);
            assert_eq!(
                store.create(ID, &value(1)).unwrap_err(),
                StoreError::new("store_deadline_exceeded", false)
            );
            assert_eq!(root.names(), vec!["format.json", "lock"]);
            store.allow_finalization(earlier + Duration::from_secs(30));
            assert_eq!(store.deadline, earlier + Duration::from_millis(2500));
            let cancelled_ceiling = earlier + Duration::from_millis(50);
            store.allow_finalization(cancelled_ceiling);
            store.allow_finalization(earlier + Duration::from_secs(30));
            assert_eq!(store.deadline, cancelled_ceiling);
            store.set_deadline(earlier);
            assert_eq!(
                store.create(ID, &value(1)).unwrap_err(),
                StoreError::new("store_deadline_exceeded", false)
            );
            assert_eq!(root.names(), vec!["format.json", "lock"]);
        }

        #[test]
        fn interrupted_staging_is_preserved_counted_and_bounded() {
            let root = Root::new();
            drop(root.open(true).unwrap());
            root.put(".tmp-run-1-a-b", b"partial");
            let mut store = root.open(false).unwrap();
            store.create(ID, &value(1)).unwrap();
            assert_eq!(fs::read(root.0.join(".tmp-run-1-a-b")).unwrap(), b"partial");
            drop(store);
            for n in 1..MAX_STAGING {
                root.put(&format!(".tmp-run-2-a-{n:x}"), b"");
            }
            let mut store = root.open(false).unwrap();
            assert_eq!(
                store.replace(ID, 1, &value(2)).unwrap_err(),
                StoreError::new("store_layout_limit", false)
            );
            drop(store);
            root.put(".tmp-run-3-a-b", b"");
            assert_eq!(root.open(false).unwrap_err().code, "store_layout_limit");
        }

        #[test]
        fn run_count_state_size_and_temporary_peak_quota_are_bounded() {
            let root = Root::new();
            let mut store = root.open(true).unwrap();
            let mut large = value(1);
            large["padding"] = Value::String("x".repeat(MAX_STATE));
            assert_eq!(
                store.create(ID, &large).unwrap_err().code,
                "store_object_too_large"
            );
            drop(store);
            for n in 0..MAX_RUNS {
                let id = format!("run-1-a-{n:x}");
                let mut state = value(1);
                state["run_id"] = serde_json::json!(id);
                root.put(&format!("{id}.json"), &serde_json::to_vec(&state).unwrap());
            }
            let mut store = root.open(false).unwrap();
            assert_eq!(
                store.create(ID, &value(1)).unwrap_err(),
                StoreError::new("store_layout_limit", false)
            );
            drop(store);
            let mut state = value(1);
            state["run_id"] = serde_json::json!("run-2-a-b");
            root.put("run-2-a-b.json", &serde_json::to_vec(&state).unwrap());
            assert_eq!(root.open(false).unwrap_err().code, "store_layout_limit");

            let root = Root::new();
            drop(root.open(true).unwrap());
            // Retained journals must remain valid when exercising the real storage quota.
            for n in 0..31 {
                let id = format!("run-1-a-{n:x}");
                let mut state = value(1);
                state["run_id"] = serde_json::json!(id);
                state["padding"] = Value::String(String::new());
                let overhead = serde_json::to_vec(&state).unwrap().len();
                state["padding"] = Value::String("x".repeat(MAX_STATE - overhead));
                root.put(&format!("{id}.json"), &serde_json::to_vec(&state).unwrap());
            }
            let mut store = root.open(false).unwrap();
            let mut near_limit = value(1);
            near_limit["padding"] = Value::String(String::new());
            let overhead = serde_json::to_vec(&near_limit).unwrap().len() + 1;
            near_limit["padding"] = Value::String("x".repeat(MAX_STATE - overhead));
            assert_eq!(
                store.create(ID, &near_limit).unwrap_err(),
                StoreError::new("store_quota_exceeded", false)
            );
            assert!(!root.0.join(format!("{ID}.json")).exists());
        }

        #[test]
        fn unsupported_state_identity_and_revision_overflow_refuse() {
            let root = Root::new();
            let mut store = root.open(true).unwrap();
            let mut wrong = value(1);
            wrong["format_version"] = serde_json::json!(2);
            assert_eq!(
                store.create(ID, &wrong).unwrap_err().code,
                "store_unsupported_format"
            );
            assert_eq!(
                store.create("../run-1-a-b", &value(1)).unwrap_err().code,
                "store_invalid_reference"
            );
            drop(store);
            root.put(
                &format!("{ID}.json"),
                &serde_json::to_vec(&value(u64::MAX)).unwrap(),
            );
            let mut store = root.open(false).unwrap();
            assert_eq!(
                store.replace(ID, u64::MAX, &value(1)).unwrap_err(),
                StoreError::new("store_revision_conflict", false)
            );
            assert_eq!(
                store.load(ID).unwrap().0["revision"],
                serde_json::json!(u64::MAX)
            );
        }

        #[test]
        fn foreign_unknown_format_blocks_open_create_and_replace_without_effects() {
            for operation in ["open", "create", "replace"] {
                let root = Root::new();
                let mut initial = root.open(true).unwrap();
                initial.create(ID, &value(1)).unwrap();
                drop(initial);
                let mut store = root.open(false).unwrap();
                let foreign_id = "run-2-ab-cd";
                let foreign = serde_json::json!({
                    "format_version":2,"run_id":foreign_id,"revision":1,"future_state":true
                });
                let foreign_bytes = serde_json::to_vec(&foreign).unwrap();
                root.put(&format!("{foreign_id}.json"), &foreign_bytes);
                let names_before = root.names();
                let original = fs::read(root.0.join(format!("{ID}.json"))).unwrap();
                let error = match operation {
                    "open" => {
                        drop(store);
                        root.open(false).unwrap_err()
                    }
                    "create" => {
                        let new_id = "run-3-ab-cd";
                        let mut next = value(1);
                        next["run_id"] = serde_json::json!(new_id);
                        store.create(new_id, &next).unwrap_err()
                    }
                    "replace" => store.replace(ID, 1, &value(2)).unwrap_err(),
                    _ => unreachable!(),
                };
                assert_eq!(
                    error,
                    StoreError::new("store_unsupported_format", false),
                    "{operation}"
                );
                assert_eq!(root.names(), names_before, "{operation}");
                assert_eq!(
                    fs::read(root.0.join(format!("{ID}.json"))).unwrap(),
                    original
                );
                assert_eq!(
                    fs::read(root.0.join(format!("{foreign_id}.json"))).unwrap(),
                    foreign_bytes
                );
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) use platform::Store;

#[cfg(not(target_os = "linux"))]
pub(super) struct Store;
#[cfg(not(target_os = "linux"))]
impl Store {
    pub(super) fn open(_: &Path, _: bool, _: Instant) -> Result<Self, StoreError> {
        Err(StoreError::new("store_platform_unavailable", false))
    }
    pub(super) fn set_deadline(&mut self, _: Instant) {}
    pub(super) fn allow_finalization(&mut self, _: Instant) {}
    pub(super) fn load(&self, _: &str) -> Result<(Value, String), StoreError> {
        Err(StoreError::new("store_platform_unavailable", false))
    }
    pub(super) fn create(&mut self, _: &str, _: &Value) -> Result<String, StoreError> {
        Err(StoreError::new("store_platform_unavailable", false))
    }
    pub(super) fn replace(&mut self, _: &str, _: u64, _: &Value) -> Result<String, StoreError> {
        Err(StoreError::new("store_platform_unavailable", false))
    }
}
