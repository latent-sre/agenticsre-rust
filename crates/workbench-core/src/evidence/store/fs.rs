//! Linux descriptor-relative primitives. No recursive path deletion or link-following fallback.
use super::StoreError;
use std::{
    ffi::{CStr, CString},
    fs::{File, Metadata},
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path},
};

#[derive(Debug)]
pub(crate) struct Directory(pub(crate) File);

pub(crate) fn io_error(error: io::Error, effects: bool) -> StoreError {
    let code = match error.raw_os_error() {
        Some(libc::ENOSPC | libc::EDQUOT) => "store_no_space",
        Some(libc::EACCES | libc::EPERM | libc::EROFS) => "store_permission_denied",
        Some(libc::ENOENT) => "store_not_found",
        Some(libc::ELOOP | libc::ENOTDIR) => "store_unsafe_entry",
        Some(libc::EEXIST) => "store_collision",
        _ => "store_io",
    };
    StoreError::new(code, effects)
}

fn c_name(name: &str) -> Result<CString, StoreError> {
    if name.is_empty() || name.contains('/') || name == ".." {
        return Err(StoreError::new("store_unsafe_entry", false));
    }
    CString::new(name).map_err(|_| StoreError::new("store_unsafe_entry", false))
}

fn from_fd(fd: i32) -> Result<File, StoreError> {
    if fd < 0 {
        Err(io_error(io::Error::last_os_error(), false))
    } else {
        // SAFETY: successful open/openat returned a newly owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

pub(crate) fn validate_metadata(
    metadata: &Metadata,
    directory: bool,
    private: bool,
) -> Result<(), StoreError> {
    // SAFETY: geteuid has no pointer arguments or mutable state.
    let uid = unsafe { libc::geteuid() };
    if (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
        || (private
            && (metadata.uid() != uid
                || metadata.mode() & 0o7777 != if directory { 0o700 } else { 0o600 }))
        || (!directory && metadata.nlink() != 1)
        || (directory && metadata.nlink() == 0)
    {
        return Err(StoreError::new("store_unsafe_entry", false));
    }
    Ok(())
}

impl Directory {
    pub(crate) fn open_at(&self, name: &str, private: bool) -> Result<Self, StoreError> {
        let name = c_name(name)?;
        // SAFETY: name is a live NUL-terminated component; self pins its parent directory.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        let file = from_fd(fd)?;
        validate_metadata(
            &file.metadata().map_err(|e| io_error(e, false))?,
            true,
            private,
        )?;
        Ok(Self(file))
    }

    pub(crate) fn create_dir(&self, name: &str) -> Result<Self, StoreError> {
        let component = c_name(name)?;
        // SAFETY: component is NUL-terminated and parent is a live pinned directory.
        let result = unsafe { libc::mkdirat(self.0.as_raw_fd(), component.as_ptr(), 0o700) };
        if result != 0 {
            return Err(io_error(io::Error::last_os_error(), false));
        }
        self.open_at(name, true).map_err(StoreError::after_effects)
    }

    pub(crate) fn open_file(&self, name: &str, writable: bool) -> Result<File, StoreError> {
        let name = c_name(name)?;
        // O_NONBLOCK ensures a malicious FIFO is refused without first blocking in openat.
        let access = if writable {
            libc::O_RDWR
        } else {
            libc::O_RDONLY
        };
        // SAFETY: the component and directory descriptor remain live through this syscall.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                access | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        let file = from_fd(fd)?;
        validate_metadata(
            &file.metadata().map_err(|e| io_error(e, false))?,
            false,
            true,
        )?;
        Ok(file)
    }

    pub(crate) fn create_file(&self, name: &str) -> Result<File, StoreError> {
        let name = c_name(name)?;
        // SAFETY: the name is a NUL-terminated component and O_EXCL never opens an existing link.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        let file = from_fd(fd)?;
        validate_metadata(
            &file.metadata().map_err(|e| io_error(e, true))?,
            false,
            true,
        )
        .map_err(StoreError::after_effects)?;
        Ok(file)
    }

    pub(crate) fn sync(&self) -> Result<(), StoreError> {
        self.0.sync_all().map_err(|e| io_error(e, true))
    }

    pub(crate) fn publish(
        &self,
        temporary: &str,
        destination: &str,
        replace: bool,
    ) -> Result<(), StoreError> {
        let temporary = c_name(temporary)?;
        let destination = c_name(destination)?;
        let flags = if replace { 0 } else { libc::RENAME_NOREPLACE };
        // SAFETY: both names are live NUL-terminated components under the same pinned directory.
        let result = unsafe {
            libc::renameat2(
                self.0.as_raw_fd(),
                temporary.as_ptr(),
                self.0.as_raw_fd(),
                destination.as_ptr(),
                flags,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io_error(io::Error::last_os_error(), true))
        }
    }

    pub(crate) fn move_to(
        &self,
        name: &str,
        directory: &Directory,
        destination: &str,
    ) -> Result<(), StoreError> {
        let name = c_name(name)?;
        let destination = c_name(destination)?;
        // SAFETY: both directory descriptors are pinned; live component names are NUL-terminated.
        let result = unsafe {
            libc::renameat2(
                self.0.as_raw_fd(),
                name.as_ptr(),
                directory.0.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io_error(io::Error::last_os_error(), true))
        }
    }

    pub(crate) fn unlink(&self, name: &str, directory: bool) -> Result<(), StoreError> {
        let name = c_name(name)?;
        let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
        // SAFETY: this removes exactly one validated component; unlinkat does not follow links.
        let result = unsafe { libc::unlinkat(self.0.as_raw_fd(), name.as_ptr(), flags) };
        if result == 0 {
            Ok(())
        } else {
            Err(io_error(io::Error::last_os_error(), true))
        }
    }

    pub(crate) fn names(&self, limit: usize) -> Result<Vec<String>, StoreError> {
        // A fresh open description avoids sharing/reusing a readdir offset between scans.
        let descriptor = self.open_at(".", false)?.0.into_raw_fd();
        // SAFETY: descriptor is uniquely owned; fdopendir consumes it only on success.
        let stream = unsafe { libc::fdopendir(descriptor) };
        if stream.is_null() {
            // SAFETY: fdopendir failed and did not consume the unique descriptor.
            unsafe { libc::close(descriptor) };
            return Err(io_error(io::Error::last_os_error(), false));
        }
        struct Stream(*mut libc::DIR);
        impl Drop for Stream {
            fn drop(&mut self) {
                // SAFETY: this is the one owner of the successfully opened directory stream.
                unsafe { libc::closedir(self.0) };
            }
        }
        let stream = Stream(stream);
        let mut names = Vec::new();
        loop {
            // SAFETY: errno is thread-local; stream remains live and is not shared.
            let entry = unsafe {
                *libc::__errno_location() = 0;
                libc::readdir(stream.0)
            };
            if entry.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(0) {
                    return Err(io_error(error, false));
                }
                break;
            }
            // SAFETY: readdir supplied a NUL-terminated name, valid until its next call.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }
                .to_str()
                .map_err(|_| StoreError::new("store_unsafe_entry", false))?;
            if name == "." || name == ".." {
                continue;
            }
            if names.len() >= limit {
                return Err(StoreError::new("store_layout_limit", false));
            }
            names.push(name.to_owned());
        }
        names.sort_unstable();
        Ok(names)
    }
}

pub(crate) fn open_root(path: &Path, create: bool) -> Result<(Directory, bool), StoreError> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => components.push(
                CString::new(name.as_bytes())
                    .ok()
                    .and_then(|name| name.into_string().ok())
                    .ok_or(StoreError::new("store_invalid_options", false))?,
            ),
            _ => return Err(StoreError::new("store_invalid_options", false)),
        }
    }
    if components.is_empty() {
        return Err(StoreError::new("store_invalid_options", false));
    }
    let start = if path.is_absolute() { c"/" } else { c"." };
    // SAFETY: fixed NUL-terminated path and no-follow directory-only flags.
    let fd = unsafe {
        libc::open(
            start.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    let mut parent = Directory(from_fd(fd)?);
    for (index, component) in components.iter().enumerate() {
        let last = index + 1 == components.len();
        match parent.open_at(component, last) {
            Ok(directory) => parent = directory,
            Err(error) if last && create && error.code == "store_not_found" => {
                let directory = parent.create_dir(component)?;
                parent.sync()?;
                return Ok((directory, true));
            }
            Err(error) => return Err(error),
        }
    }
    Ok((parent, false))
}

pub(crate) fn lock(directory: &Directory, create: bool) -> Result<(File, bool), StoreError> {
    let (file, created) = match directory.open_file("lock", true) {
        Ok(file) => (file, false),
        Err(error) if create && error.code == "store_not_found" => {
            match directory.create_file("lock") {
                Ok(file) => (file, true),
                Err(error) if error.code == "store_collision" => {
                    (directory.open_file("lock", true)?, false)
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) => return Err(error),
    };
    if file.metadata().map_err(|e| io_error(e, created))?.len() != 0 {
        return Err(StoreError::new("store_corrupt", created));
    }
    // SAFETY: file owns a live descriptor, and flock changes no file bytes.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Err(StoreError::new("store_busy", created));
        }
        return Err(io_error(error, created));
    }
    if created {
        file.sync_all().map_err(|e| io_error(e, true))?;
        directory.sync()?;
    }
    Ok((file, created))
}

pub(crate) fn read(directory: &Directory, name: &str, max: usize) -> Result<Vec<u8>, StoreError> {
    let mut file = directory.open_file(name, false)?;
    let before = file.metadata().map_err(|e| io_error(e, false))?;
    if before.len() > max as u64 {
        return Err(StoreError::new("store_object_too_large", false));
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error(e, false))?;
    let after = file.metadata().map_err(|e| io_error(e, false))?;
    validate_metadata(&after, false, true)?;
    if bytes.len() > max || after.len() != bytes.len() as u64 || before.len() != after.len() {
        return Err(StoreError::new("store_corrupt", false));
    }
    Ok(bytes)
}
