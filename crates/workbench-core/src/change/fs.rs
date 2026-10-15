//! Descriptor-relative, leased replacement of one explicitly granted Linux file.
//!
//! The lease serializes cooperating writers. Atomic rename is not compare-and-swap against
//! arbitrary writers with the same uid. Artifacts are retained on every uncertain outcome.
use crate::evidence::store::{
    StoreError,
    fs::{self as store_fs, Directory},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, Metadata},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Component, Path, PathBuf},
};
const MAX_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    pub dev: u64,
    pub ino: u64,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub nlink: u64,
    pub size: u64,
    pub ctime_sec: i64,
    pub ctime_nsec: i64,
    pub mtime_sec: i64,
    pub mtime_nsec: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    pub parent: Identity,
    pub file: Identity,
    pub sha256: String,
}

impl Snapshot {
    /// Compare the target precondition while allowing unrelated directory-entry bookkeeping.
    pub fn matches(&self, expected: &Self) -> bool {
        snapshot_matches(self, expected)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Prepared {
    pub temporary_name: String,
    pub snapshot: Snapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Observation {
    Original,
    Replacement,
    Unknown,
}

#[derive(Debug)]
pub(super) struct Target {
    path: PathBuf,
    name: String,
    parent: Directory,
    parent_identity: Identity,
    lease: File,
    lease_name: String,
}

fn store_error(error: StoreError) -> &'static str {
    match error.code {
        "store_not_found" => "change_target_missing",
        "store_collision" => "change_artifact_collision",
        "store_no_space" => "change_no_space",
        "store_permission_denied" => "change_permission_denied",
        "store_unsafe_entry" | "store_invalid_options" => "change_unsafe_target",
        _ => "change_filesystem_io",
    }
}
fn io_error(error: std::io::Error) -> &'static str {
    store_error(store_fs::io_error(error, false))
}
fn identity(metadata: &Metadata) -> Identity {
    Identity {
        dev: metadata.dev(),
        ino: metadata.ino(),
        uid: metadata.uid(),
        gid: metadata.gid(),
        mode: metadata.mode(),
        nlink: metadata.nlink(),
        size: metadata.len(),
        ctime_sec: metadata.ctime(),
        ctime_nsec: metadata.ctime_nsec(),
        mtime_sec: metadata.mtime(),
        mtime_nsec: metadata.mtime_nsec(),
    }
}
fn parent_matches(a: &Identity, b: &Identity) -> bool {
    // Creating the lease/prepared entry changes directory timestamps and size legitimately.
    a.dev == b.dev
        && a.ino == b.ino
        && a.uid == b.uid
        && a.gid == b.gid
        && a.mode == b.mode
        && a.nlink == b.nlink
}
fn snapshot_matches(a: &Snapshot, b: &Snapshot) -> bool {
    parent_matches(&a.parent, &b.parent) && a.file == b.file && a.sha256 == b.sha256
}
fn replacement_matches(a: &Snapshot, b: &Snapshot) -> bool {
    let mut observed = a.file.clone();
    // Linux rename may advance ctime. All other file metadata and exact bytes must agree.
    let rename_ctime =
        (observed.ctime_sec, observed.ctime_nsec) >= (b.file.ctime_sec, b.file.ctime_nsec);
    observed.ctime_sec = b.file.ctime_sec;
    observed.ctime_nsec = b.file.ctime_nsec;
    parent_matches(&a.parent, &b.parent)
        && observed == b.file
        && a.sha256 == b.sha256
        && rename_ctime
}
fn digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}

impl Target {
    pub fn open(path: &Path) -> Result<Self, &'static str> {
        if path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            return Err("change_unsafe_target");
        }
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().map_err(io_error)?.join(path)
        };
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .ok_or("change_unsafe_target")?
            .to_owned();
        let parent_path = path.parent().ok_or("change_unsafe_target")?;
        let parent = store_fs::open_root(parent_path, false)
            .map_err(store_error)?
            .0;
        let parent_identity = identity(&parent.0.metadata().map_err(io_error)?);
        let lease_name = format!(
            ".workbench-change-{}.lock",
            digest(name.as_bytes()).trim_start_matches("sha256:")
        );
        let lease = match parent.open_file(&lease_name, true) {
            Ok(file) => file,
            Err(error) if error.code == "store_not_found" => {
                match parent.create_file(&lease_name) {
                    Ok(file) => {
                        file.sync_all().map_err(io_error)?;
                        parent.sync().map_err(store_error)?;
                        file
                    }
                    Err(error) if error.code == "store_collision" => {
                        parent.open_file(&lease_name, true).map_err(store_error)?
                    }
                    Err(error) => return Err(store_error(error)),
                }
            }
            Err(error) => return Err(store_error(error)),
        };
        if lease.metadata().map_err(io_error)?.len() != 0 {
            return Err("change_unsafe_lease");
        }
        // SAFETY: the descriptor is live and exclusively owned; the nonblocking flock writes no bytes.
        if unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = std::io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
                "change_target_busy"
            } else {
                io_error(error)
            });
        }
        let target = Self {
            path,
            name,
            parent,
            parent_identity,
            lease,
            lease_name,
        };
        target.snapshot()?;
        Ok(target)
    }

    fn check_parent(&self) -> Result<(), &'static str> {
        let current = store_fs::open_root(self.path.parent().ok_or("change_unsafe_target")?, false)
            .map_err(store_error)?
            .0;
        let pinned = self.parent.0.metadata().map_err(io_error)?;
        store_fs::validate_metadata(&pinned, true, true).map_err(store_error)?;
        if !parent_matches(&identity(&pinned), &self.parent_identity)
            || !parent_matches(
                &identity(&current.0.metadata().map_err(io_error)?),
                &self.parent_identity,
            )
        {
            return Err("change_parent_changed");
        }
        let named_lease = self
            .parent
            .open_file(&self.lease_name, false)
            .map_err(store_error)?;
        if identity(&named_lease.metadata().map_err(io_error)?)
            != identity(&self.lease.metadata().map_err(io_error)?)
        {
            return Err("change_lease_changed");
        }
        Ok(())
    }

    fn read_named(&self, name: &str) -> Result<(Snapshot, Vec<u8>), &'static str> {
        self.check_parent()?;
        let mut file = self.parent.open_file(name, false).map_err(store_error)?;
        let before = identity(&file.metadata().map_err(io_error)?);
        if before.size > MAX_BYTES as u64 {
            return Err("change_target_too_large");
        }
        let mut bytes = Vec::with_capacity(before.size as usize);
        (&mut file)
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        let after_metadata = file.metadata().map_err(io_error)?;
        store_fs::validate_metadata(&after_metadata, false, true).map_err(store_error)?;
        let after = identity(&after_metadata);
        let named = self.parent.open_file(name, false).map_err(store_error)?;
        if before != after
            || after != identity(&named.metadata().map_err(io_error)?)
            || after.size != bytes.len() as u64
            || bytes.len() > MAX_BYTES
        {
            return Err("change_target_changed");
        }
        self.check_parent()?;
        Ok((
            Snapshot {
                parent: self.parent_identity.clone(),
                file: after,
                sha256: digest(&bytes),
            },
            bytes,
        ))
    }

    pub fn snapshot(&self) -> Result<(Snapshot, Vec<u8>), &'static str> {
        self.read_named(&self.name)
    }

    /// Establish durability again while holding the same target lease used for observation.
    pub fn sync(&self) -> Result<(), &'static str> {
        self.check_parent()?;
        self.parent
            .open_file(&self.name, false)
            .map_err(store_error)?
            .sync_all()
            .map_err(io_error)?;
        self.parent.sync().map_err(store_error)?;
        self.check_parent()
    }

    pub fn prepare(
        &self,
        bytes: &[u8],
        expected: &Snapshot,
        unique: &str,
    ) -> Result<Prepared, &'static str> {
        if bytes.len() > MAX_BYTES {
            return Err("change_target_too_large");
        }
        if unique.is_empty()
            || unique.len() > 128
            || !unique
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("change_invalid_artifact_name");
        }
        if !snapshot_matches(&self.snapshot()?.0, expected) {
            return Err("change_target_changed");
        }
        let temporary_name = format!(
            ".workbench-change-{}-{}.tmp",
            digest(self.name.as_bytes()).trim_start_matches("sha256:"),
            unique
        );
        let mut file = self
            .parent
            .create_file(&temporary_name)
            .map_err(store_error)?;
        file.write_all(bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        self.parent.sync().map_err(store_error)?;
        let snapshot = self.read_named(&temporary_name)?.0;
        if snapshot.sha256 != digest(bytes)
            || snapshot.file != identity(&file.metadata().map_err(io_error)?)
        {
            return Err("change_prepared_changed");
        }
        if !snapshot_matches(&self.snapshot()?.0, expected) {
            return Err("change_target_changed");
        }
        Ok(Prepared {
            temporary_name,
            snapshot,
        })
    }

    pub fn replace(&self, expected: &Snapshot, prepared: &Prepared) -> Result<(), &'static str> {
        if prepared.temporary_name == self.name
            || !prepared.temporary_name.starts_with(&format!(
                ".workbench-change-{}-",
                digest(self.name.as_bytes()).trim_start_matches("sha256:")
            ))
            || !prepared.temporary_name.ends_with(".tmp")
        {
            return Err("change_invalid_artifact_name");
        }
        if !snapshot_matches(
            &self.read_named(&prepared.temporary_name)?.0,
            &prepared.snapshot,
        ) {
            return Err("change_prepared_changed");
        }
        if !snapshot_matches(&self.snapshot()?.0, expected) {
            return Err("change_target_changed");
        }
        self.check_parent()?;
        // Both rechecked names and the atomic replacement stay under the same pinned descriptor.
        self.parent
            .publish(&prepared.temporary_name, &self.name, true)
            .map_err(store_error)?;
        self.parent.sync().map_err(store_error)?;
        Ok(())
    }

    pub fn observe(
        &self,
        original: &Snapshot,
        prepared: &Prepared,
    ) -> Result<Observation, &'static str> {
        self.check_parent()?;
        let observed = match self.snapshot() {
            Ok((snapshot, _)) => snapshot,
            Err("change_parent_changed" | "change_lease_changed") => {
                return Err("change_parent_changed");
            }
            Err(_) => return Ok(Observation::Unknown),
        };
        Ok(if snapshot_matches(&observed, original) {
            Observation::Original
        } else if replacement_matches(&observed, &prepared.snapshot) {
            Observation::Replacement
        } else {
            Observation::Unknown
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        path: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "change-fs-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            let path = root.join("rules.yaml");
            private_file(&path, b"original");
            Self { root, path }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    fn private_file(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn prepare_replace_and_reconcile_keep_identity() {
        let f = Fixture::new();
        let target = Target::open(&f.path).unwrap();
        let (original, bytes) = target.snapshot().unwrap();
        assert_eq!(bytes, b"original");
        let prepared = target
            .prepare(b"replacement", &original, "positive")
            .unwrap();
        assert_eq!(fs::read(&f.path).unwrap(), b"original");
        assert_eq!(
            target.observe(&original, &prepared).unwrap(),
            Observation::Original
        );
        let persisted: Prepared =
            serde_json::from_slice(&serde_json::to_vec(&prepared).unwrap()).unwrap();
        target.replace(&original, &persisted).unwrap();
        assert_eq!(fs::read(&f.path).unwrap(), b"replacement");
        assert_eq!(
            target.snapshot().unwrap().0.file.ino,
            prepared.snapshot.file.ino
        );
        assert_eq!(
            target.observe(&original, &persisted).unwrap(),
            Observation::Replacement
        );
        assert!(target.replace(&original, &persisted).is_err());
        drop(target);
        assert_eq!(
            Target::open(&f.path)
                .unwrap()
                .observe(&original, &persisted)
                .unwrap(),
            Observation::Replacement
        );
    }

    #[test]
    fn original_mutation_refuses_dispatch() {
        let f = Fixture::new();
        let target = Target::open(&f.path).unwrap();
        let original = target.snapshot().unwrap().0;
        let prepared = target
            .prepare(b"replacement", &original, "changed")
            .unwrap();
        fs::write(&f.path, b"changed!").unwrap();
        assert!(target.replace(&original, &prepared).is_err());
        assert_eq!(
            target.observe(&original, &prepared).unwrap(),
            Observation::Unknown
        );
        assert_eq!(fs::read(&f.path).unwrap(), b"changed!");
    }

    #[test]
    fn replacement_bytes_on_wrong_inode_are_unknown() {
        let f = Fixture::new();
        let target = Target::open(&f.path).unwrap();
        let original = target.snapshot().unwrap().0;
        let prepared = target
            .prepare(b"replacement", &original, "wrong-inode")
            .unwrap();
        fs::remove_file(&f.path).unwrap();
        private_file(&f.path, b"replacement");
        assert_eq!(
            target.observe(&original, &prepared).unwrap(),
            Observation::Unknown
        );
        assert!(target.replace(&original, &prepared).is_err());
    }

    #[test]
    fn symlinks_hardlinks_and_fifo_are_refused() {
        let f = Fixture::new();
        let link = f.root.join("link");
        symlink(&f.path, &link).unwrap();
        assert!(Target::open(&link).is_err());
        fs::hard_link(&f.path, f.root.join("hard")).unwrap();
        assert!(Target::open(&f.path).is_err());
        fs::remove_file(f.root.join("hard")).unwrap();
        fs::remove_file(&f.path).unwrap();
        let name = std::ffi::CString::new(f.path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: name is a live NUL-terminated path; the private fixture owns this pathname.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(Target::open(&f.path).is_err());
    }

    #[test]
    fn substituted_directory_is_refused() {
        let f = Fixture::new();
        let target = Target::open(&f.path).unwrap();
        let original = target.snapshot().unwrap().0;
        let prepared = target
            .prepare(b"replacement", &original, "directory")
            .unwrap();
        let moved = f.root.with_extension("moved");
        fs::rename(&f.root, &moved).unwrap();
        fs::create_dir(&f.root).unwrap();
        fs::set_permissions(&f.root, fs::Permissions::from_mode(0o700)).unwrap();
        private_file(&f.path, b"new directory");
        assert!(target.snapshot().is_err());
        assert!(target.replace(&original, &prepared).is_err());
        assert!(target.observe(&original, &prepared).is_err());
        assert_eq!(fs::read(moved.join("rules.yaml")).unwrap(), b"original");
        fs::remove_dir_all(moved).unwrap();
    }

    #[test]
    fn replaced_or_missing_prepared_file_refuses_dispatch() {
        for missing in [true, false] {
            let f = Fixture::new();
            let target = Target::open(&f.path).unwrap();
            let original = target.snapshot().unwrap().0;
            let prepared = target.prepare(b"replacement", &original, "temp").unwrap();
            let temporary = f.root.join(&prepared.temporary_name);
            fs::rename(&temporary, f.root.join("retained")).unwrap();
            if !missing {
                private_file(&temporary, b"replacement");
            }
            assert!(target.replace(&original, &prepared).is_err());
            assert_eq!(fs::read(&f.path).unwrap(), b"original");
            assert_eq!(
                target.observe(&original, &prepared).unwrap(),
                Observation::Original
            );
        }
    }

    #[test]
    fn target_specific_nonblocking_lease_is_held_for_lifetime() {
        let f = Fixture::new();
        let target = Target::open(&f.path).unwrap();
        assert_eq!(Target::open(&f.path).unwrap_err(), "change_target_busy");
        let other = f.root.join("other");
        private_file(&other, b"other");
        let other_target = Target::open(&other).unwrap();
        drop(target);
        Target::open(&f.path).unwrap();
        drop(other_target);
    }

    #[test]
    fn parent_bookkeeping_does_not_invalidate_target_precondition() {
        let f = Fixture::new();
        let target = Target::open(&f.path).unwrap();
        let original = target.snapshot().unwrap().0;
        drop(target);
        private_file(&f.root.join("unrelated"), b"unrelated");
        let target = Target::open(&f.path).unwrap();
        assert!(target.snapshot().unwrap().0.matches(&original));
        let prepared = target
            .prepare(b"replacement", &original, "parent-bookkeeping")
            .unwrap();
        target.replace(&original, &prepared).unwrap();
        assert_eq!(
            target.observe(&original, &prepared).unwrap(),
            Observation::Replacement
        );
    }

    #[test]
    fn original_bytes_on_wrong_inode_are_unknown() {
        let f = Fixture::new();
        let target = Target::open(&f.path).unwrap();
        let original = target.snapshot().unwrap().0;
        let prepared = target
            .prepare(b"replacement", &original, "original-inode")
            .unwrap();
        fs::rename(&f.path, f.root.join("retained-original")).unwrap();
        private_file(&f.path, b"original");
        assert_eq!(
            target.observe(&original, &prepared).unwrap(),
            Observation::Unknown
        );
        assert!(target.replace(&original, &prepared).is_err());
    }

    #[test]
    fn unsafe_metadata_after_prepare_refuses_dispatch() {
        for temporary in [true, false] {
            let f = Fixture::new();
            let target = Target::open(&f.path).unwrap();
            let original = target.snapshot().unwrap().0;
            let prepared = target
                .prepare(b"replacement", &original, "metadata")
                .unwrap();
            let changed = if temporary {
                f.root.join(&prepared.temporary_name)
            } else {
                f.path.clone()
            };
            fs::hard_link(&changed, f.root.join("hardlink")).unwrap();
            assert!(target.replace(&original, &prepared).is_err());
            assert_eq!(fs::read(&f.path).unwrap(), b"original");
        }
    }

    #[test]
    fn replaced_lease_refuses_all_observation_and_dispatch() {
        let f = Fixture::new();
        let target = Target::open(&f.path).unwrap();
        let original = target.snapshot().unwrap().0;
        let prepared = target.prepare(b"replacement", &original, "lease").unwrap();
        let lease = f.root.join(&target.lease_name);
        fs::rename(&lease, f.root.join("retained-lease")).unwrap();
        private_file(&lease, b"");
        assert_eq!(target.snapshot().unwrap_err(), "change_lease_changed");
        assert!(target.replace(&original, &prepared).is_err());
        assert!(target.observe(&original, &prepared).is_err());
        assert_eq!(fs::read(&f.path).unwrap(), b"original");
    }

    #[test]
    fn preparation_rechecks_original_and_retains_collision() {
        let f = Fixture::new();
        let target = Target::open(&f.path).unwrap();
        let original = target.snapshot().unwrap().0;
        let prepared = target
            .prepare(b"replacement", &original, "collision")
            .unwrap();
        assert_eq!(
            target
                .prepare(b"other", &original, "collision")
                .unwrap_err(),
            "change_artifact_collision"
        );
        assert_eq!(
            fs::read(f.root.join(&prepared.temporary_name)).unwrap(),
            b"replacement"
        );
        fs::write(&f.path, b"changed").unwrap();
        assert_eq!(
            target.prepare(b"other", &original, "changed").unwrap_err(),
            "change_target_changed"
        );
        assert!(target.prepare(b"other", &original, "../unsafe").is_err());
    }

    #[test]
    fn oversize_or_nonprivate_targets_are_refused() {
        let f = Fixture::new();
        fs::write(&f.path, vec![0; 32769]).unwrap();
        assert!(Target::open(&f.path).is_err());
        fs::write(&f.path, b"original").unwrap();
        fs::set_permissions(&f.path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Target::open(&f.path).is_err());
        fs::set_permissions(&f.path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&f.root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Target::open(&f.path).is_err());
    }
}
