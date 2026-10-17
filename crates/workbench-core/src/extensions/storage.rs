use super::*;
use crate::evidence::store::{
    StoreError,
    fs::{self, Directory},
};
use package::{MAX_PACKAGE, Package};
use std::{
    collections::BTreeSet,
    ffi::CString,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path},
};
const MAX_PACKAGES: usize = 32;
const MAX_STORE: usize = 64 * 1024 * 1024;
const FORMAT: &[u8] = b"{\"format_version\":1,\"kind\":\"save-extension-store\"}\n";
fn store_error(error: StoreError) -> Fault {
    Fault {
        code: match error.code {
            "store_not_found" => "extension_store_not_found",
            "store_busy" => "extension_store_busy",
            "store_no_space" => "extension_store_no_space",
            "store_permission_denied" => "extension_store_permission_denied",
            _ => "extension_recovery_required",
        },
        effects: error.effects,
    }
}
fn read(
    mut file: File,
    control: &RunControl,
    deadline: Instant,
    private: bool,
    max: usize,
) -> Result<Vec<u8>, Fault> {
    let bad = || {
        Fault::new(if private {
            "extension_recovery_required"
        } else {
            "extension_source_unsafe"
        })
    };
    let before = file.metadata().map_err(|_| bad())?;
    if !before.is_file() || before.nlink() != 1 {
        return Err(bad());
    }
    if private {
        fs::validate_metadata(&before, false, true).map_err(store_error)?;
    }
    if before.len() > max as u64 {
        return Err(Fault::new(if private {
            "extension_recovery_required"
        } else {
            "extension_package_too_large"
        }));
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    let mut buffer = [0; 64 * 1024];
    loop {
        active(control, deadline)?;
        let count = file.read(&mut buffer).map_err(|_| bad())?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > max {
            return Err(bad());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let after = file.metadata().map_err(|_| bad())?;
    if after.nlink() != 1
        || before.len() != after.len()
        || bytes.len() as u64 != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(bad());
    }
    if private {
        fs::validate_metadata(&after, false, true).map_err(store_error)?;
    }
    Ok(bytes)
}
pub(super) fn source(
    path: &Path,
    control: &RunControl,
    deadline: Instant,
    max: usize,
    private: bool,
) -> Result<Vec<u8>, Fault> {
    active(control, deadline)?;
    let bad = || Fault::new("extension_source_unsafe");
    let mut parent = Directory(
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open("/")
            .map_err(|_| bad())?,
    );
    let components: Vec<_> = path.components().collect();
    for (index, component) in components.iter().enumerate() {
        active(control, deadline)?;
        match component {
            Component::RootDir => {}
            Component::Normal(name) if index + 1 == components.len() => {
                let name = CString::new(name.as_bytes()).map_err(|_| bad())?;
                // SAFETY: pinned directory and live single component; nonblocking refuses FIFOs without waiting.
                let fd = unsafe {
                    libc::openat(
                        parent.0.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if fd < 0 {
                    return Err(bad());
                }
                // SAFETY: successful openat transferred unique ownership.
                return read(
                    unsafe { File::from_raw_fd(fd) },
                    control,
                    deadline,
                    private,
                    max,
                );
            }
            Component::Normal(name) => {
                parent = parent
                    .open_at(name.to_str().ok_or_else(bad)?, false)
                    .map_err(|_| bad())?
            }
            _ => return Err(bad()),
        }
    }
    Err(bad())
}
pub(super) struct Store {
    directory: Directory,
    _lock: File,
    pub packages: Vec<Package>,
    bytes: usize,
    effects: bool,
}
impl Store {
    pub fn open(
        path: &Path,
        create: bool,
        control: &RunControl,
        deadline: Instant,
    ) -> Result<Self, Fault> {
        active(control, deadline)?;
        if !path.is_absolute()
            || path
                .to_str()
                .is_none_or(|s| s.len() > 4096 || s.chars().any(char::is_control))
        {
            return Err(Fault::new("extension_invalid_store_path"));
        }
        let (directory, created) = fs::open_root(path, create).map_err(store_error)?;
        let mut effects = created;
        let loaded = (|| {
            let initial = directory.names(MAX_PACKAGES + 3).map_err(store_error)?;
            let initialize = initial.is_empty() && create;
            if !initialize
                && (!initial.iter().any(|n| n == "format.json")
                    || !initial.iter().any(|n| n == "lock"))
            {
                return Err(Fault::new("extension_recovery_required"));
            }
            let (lock, lock_created) = fs::lock(&directory, initialize).map_err(store_error)?;
            effects |= lock_created;
            if initialize {
                active(control, deadline)?;
                // Only the process that created this lock may initialize. Concurrent
                // starters observe the incomplete layout and refuse rather than repair it.
                if !lock_created || directory.names(2).map_err(store_error)? != ["lock"] {
                    return Err(Fault::new("extension_recovery_required"));
                }
                publish(&directory, "format.json", FORMAT, control, deadline)?;
            }
            let names = directory.names(MAX_PACKAGES + 2).map_err(store_error)?;
            if fs::read(&directory, "format.json", 128).map_err(store_error)? != FORMAT {
                return Err(Fault::new("extension_recovery_required"));
            }
            let mut packages = Vec::new();
            let mut identities = BTreeSet::new();
            let mut bytes = FORMAT.len();
            for name in names {
                active(control, deadline)?;
                if matches!(name.as_str(), "lock" | "format.json") {
                    continue;
                }
                let Some(hash) = name
                    .strip_prefix("package-")
                    .and_then(|n| n.strip_suffix(".json"))
                else {
                    return Err(Fault::new("extension_recovery_required"));
                };
                if !digest_string(&format!("sha256:{hash}")) {
                    return Err(Fault::new("extension_recovery_required"));
                }
                let raw = read(
                    directory.open_file(&name, false).map_err(store_error)?,
                    control,
                    deadline,
                    true,
                    MAX_PACKAGE,
                )?;
                bytes += raw.len();
                if bytes > MAX_STORE || packages.len() >= MAX_PACKAGES {
                    return Err(Fault::new("extension_recovery_required"));
                }
                let package = package::decode(raw, control, deadline).map_err(|e| {
                    if e.code.starts_with("invalid_") || e.code == "extension_package_too_large" {
                        Fault::new("extension_recovery_required")
                    } else {
                        e
                    }
                })?;
                if package.digest != format!("sha256:{hash}")
                    || !identities.insert((package.identity().0.to_owned(), package.identity().1))
                {
                    return Err(Fault::new("extension_recovery_required"));
                }
                packages.push(package);
            }
            Ok((lock, packages, bytes))
        })();
        let (lock, packages, bytes) =
            loaded.map_err(|e: Fault| if effects { e.after_effects() } else { e })?;
        Ok(Self {
            directory,
            _lock: lock,
            packages,
            bytes,
            effects,
        })
    }
    fn install(
        &mut self,
        package: Package,
        control: &RunControl,
        deadline: Instant,
    ) -> Result<&'static str, Fault> {
        active(control, deadline)?;
        for installed in &self.packages {
            if installed.digest == package.digest {
                return Ok("already_installed");
            }
            if installed.identity() == package.identity() {
                return Err(Fault::new("extension_package_conflict"));
            }
        }
        if self.packages.len() >= MAX_PACKAGES || self.bytes + package.bytes.len() > MAX_STORE {
            return Err(Fault::new("extension_quota_exceeded"));
        }
        let name = format!("package-{}.json", &package.digest[7..]);
        publish(&self.directory, &name, &package.bytes, control, deadline)?;
        self.effects = true;
        self.bytes += package.bytes.len();
        self.packages.push(package);
        self.packages.sort_by(|a, b| a.digest.cmp(&b.digest));
        Ok("installed")
    }
}
fn publish(
    directory: &Directory,
    destination: &str,
    bytes: &[u8],
    control: &RunControl,
    deadline: Instant,
) -> Result<(), Fault> {
    active(control, deadline)?;
    let pending = crate::result::new_id("pending");
    let mut file = directory.create_file(&pending).map_err(store_error)?;
    let published = (|| {
        for chunk in bytes.chunks(64 * 1024) {
            active(control, deadline)?;
            file.write_all(chunk)
                .map_err(|_| Fault::new("extension_store_write_failed"))?;
        }
        file.sync_all()
            .map_err(|_| Fault::new("extension_store_write_failed"))?;
        active(control, deadline)?;
        #[cfg(test)]
        if FAIL_BEFORE_PUBLISH.with(|flag| flag.replace(false)) {
            return Err(Fault::new("extension_test_write_failed"));
        }
        directory
            .publish(&pending, destination, false)
            .map_err(store_error)?;
        directory.sync().map_err(store_error)?;
        Ok(())
    })();
    // Preserve any interrupted staging bytes for explicit recovery, never silently
    // delete an unknown or partial file. The next open refuses the pending name.
    published.map_err(Fault::after_effects)
}
#[cfg(test)]
thread_local! { pub(super) static FAIL_BEFORE_PUBLISH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
pub(super) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    deadline: Instant,
    result: &mut OperationResult,
) -> Result<(), Fault> {
    active(control, deadline)?;
    let package = if request.operation == "extension.list" {
        None
    } else {
        let raw = source(
            Path::new(request.inputs["package"].as_str().unwrap()),
            control,
            deadline,
            MAX_PACKAGE,
            false,
        )?;
        let package = package::decode(raw, control, deadline)?;
        if request
            .inputs
            .get("expected_package_sha256")
            .is_some_and(|pin| pin != &package.digest)
        {
            return Err(Fault::new("extension_package_digest_mismatch"));
        }
        Some(package)
    };
    if request.operation == "extension.inspect" {
        result.data["packages"] = json!([package.unwrap().description()]);
        result.effect_outcome = "not_applicable".into();
        return Ok(());
    }
    let policy = policy::Policy::load(context, control, deadline)?;
    let path = context
        .extension_state_dir
        .as_deref()
        .ok_or_else(|| Fault::new("extension_store_required"))?;
    let mut store = Store::open(
        path,
        request.operation == "extension.install",
        control,
        deadline,
    )?;
    if store.effects {
        result.effect_outcome = "succeeded".into();
    }
    if let Some(package) = package {
        let action = store
            .install(package, control, deadline)
            .map_err(|e| if store.effects { e.after_effects() } else { e })?;
        result.data["action"] = json!(action);
        result.effect_outcome = if action == "installed" {
            "succeeded"
        } else {
            "not_applicable"
        }
        .into();
    } else {
        result.effect_outcome = "not_applicable".into();
    }
    result.data["installed_count"] = json!(store.packages.len());
    policy::inventory(&store.packages, policy.as_ref(), result);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_publication_preserves_staging_and_refuses_subsequent_inventory() {
        let path = std::env::temp_dir().join(crate::result::new_id("extension-interrupt"));
        let control = RunControl::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        let store = Store::open(&path, true, &control, deadline).unwrap();
        FAIL_BEFORE_PUBLISH.with(|flag| flag.set(true));
        let fault = publish(
            &store.directory,
            "package-never-published.json",
            b"partial bytes",
            &control,
            deadline,
        )
        .unwrap_err();
        assert_eq!(fault.code, "extension_test_write_failed");
        assert!(fault.effects);
        let names = store.directory.names(10).unwrap();
        assert_eq!(names.len(), 3);
        let pending = names
            .iter()
            .find(|name| name.starts_with("pending-"))
            .unwrap();
        assert_eq!(
            fs::read(&store.directory, pending, 1024).unwrap(),
            b"partial bytes"
        );
        assert!(!path.join("package-never-published.json").exists());
        drop(store);
        assert_eq!(
            Store::open(&path, false, &control, deadline)
                .err()
                .unwrap()
                .code,
            "extension_recovery_required"
        );
        assert_eq!(std::fs::read(path.join(pending)).unwrap(), b"partial bytes");
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn cancellation_or_expiry_before_publication_creates_no_pending_file() {
        let path = std::env::temp_dir().join(crate::result::new_id("extension-cancel"));
        let control = RunControl::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        let store = Store::open(&path, true, &control, deadline).unwrap();
        let names = store.directory.names(10).unwrap();
        let expired = publish(
            &store.directory,
            "never",
            b"bytes",
            &control,
            Instant::now(),
        )
        .unwrap_err();
        assert_eq!(expired.code, "extension_deadline_exceeded");
        assert!(!expired.effects);
        control.signal.store(15, Ordering::Relaxed);
        let cancelled =
            publish(&store.directory, "never", b"bytes", &control, deadline).unwrap_err();
        assert_eq!(cancelled.code, "extension_cancelled");
        assert!(!cancelled.effects);
        assert_eq!(store.directory.names(10).unwrap(), names);
        drop(store);
        std::fs::remove_dir_all(path).unwrap();
    }
}
