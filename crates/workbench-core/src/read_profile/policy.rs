use crate::{
    RunControl,
    request::{Problem, bounded, parse_bounded_json},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{FileExt, MetadataExt},
    },
    path::{Component, Path},
    sync::atomic::Ordering,
    time::Instant,
};

pub(super) const MAX_FILE: u64 = 64 * 1024 * 1024;
pub(super) const RUNTIME: [&str; 6] = [
    "/lib64/ld-linux-x86-64.so.2",
    "/lib/x86_64-linux-gnu/libc.so.6",
    "/lib/x86_64-linux-gnu/libm.so.6",
    "/lib/x86_64-linux-gnu/libpcre2-8.so.0",
    "/lib/x86_64-linux-gnu/libz.so.1",
    "/lib/x86_64-linux-gnu/libgcc_s.so.1",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub path: String,
    pub sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Executables {
    pub git: Binding,
    pub rg: Binding,
    pub bwrap: Binding,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Root {
    pub id: String,
    pub path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Policy {
    pub version: u64,
    pub profile: String,
    pub roots: Vec<Root>,
    pub executables: Executables,
}

pub(super) fn error(code: &'static str) -> Problem {
    Problem::invalid(
        code,
        "The explicit read policy, command, root or descriptor binding could not be admitted; no command was dispatched.",
    )
}
pub(super) fn check(control: &RunControl, deadline: Instant) -> Result<(), Problem> {
    if control.signal.load(Ordering::Relaxed) != 0 {
        return Err(error("cancelled"));
    }
    if Instant::now() >= deadline {
        return Err(error("timed_out"));
    }
    Ok(())
}
pub(super) fn absolute(path: &str) -> bool {
    bounded(path, 2, 1024)
        && Path::new(path).is_absolute()
        && !path.contains(['\\', '\n', '\r'])
        && !path.ends_with('/')
        && !path.contains("//")
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
        && !path.split('/').any(|c| matches!(c, "." | ".."))
}
fn encoded_digest(bytes: &[u8]) -> String {
    let mut value = String::from("sha256:");
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut value, "{byte:02x}").expect("string write");
    }
    value
}
pub(super) fn digest(bytes: &[u8]) -> String {
    encoded_digest(&Sha256::digest(bytes))
}

// openat2 resolves the complete pathname under the selected directory descriptor.
// There is no realpath-then-reopen window and no /proc magic-link traversal.
pub(super) fn open_at(
    directory: i32,
    path: &str,
    flags: i32,
    beneath: bool,
) -> std::io::Result<File> {
    let name = CString::new(path).map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    // SAFETY: open_how consists solely of integer fields; all-zero is the kernel's default.
    let mut how: libc::open_how = unsafe { std::mem::zeroed() };
    how.flags = (flags
        | libc::O_CLOEXEC
        | if flags & libc::O_PATH == 0 {
            libc::O_NONBLOCK
        } else {
            0
        }) as u64;
    how.resolve = libc::RESOLVE_NO_SYMLINKS
        | libc::RESOLVE_NO_MAGICLINKS
        | if beneath { libc::RESOLVE_BENEATH } else { 0 };
    // SAFETY: name and how are valid for the syscall; the returned fd is newly owned.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            directory,
            name.as_ptr(),
            &how,
            std::mem::size_of::<libc::open_how>(),
        )
    } as i32;
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: a successful openat2 returned this unique descriptor.
    let opened = unsafe { File::from_raw_fd(fd) };
    // Never let a policy fd alias stdio, which Command replaces before pre_exec.
    // SAFETY: fcntl duplicates the live fd and does not retain borrowed references.
    let duplicate = unsafe { libc::fcntl(opened.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    if duplicate < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: F_DUPFD_CLOEXEC returned a distinct owned descriptor.
    Ok(unsafe { File::from_raw_fd(duplicate) })
}

fn trusted(file: &File, maximum: u64, executable: bool) -> Result<(), Problem> {
    let meta = file
        .metadata()
        .map_err(|_| error("read_binding_unavailable"))?;
    // SAFETY: geteuid has no arguments or memory effects relevant to Rust safety.
    let uid = unsafe { libc::geteuid() };
    if !meta.is_file()
        || meta.len() > maximum
        || meta.mode() & 0o6022 != 0
        || (meta.uid() != 0 && meta.uid() != uid)
        || (executable && meta.mode() & 0o111 == 0)
    {
        return Err(error("untrusted_read_binding"));
    }
    Ok(())
}

pub(super) fn load(path: &Path) -> Result<(Policy, String), Problem> {
    let text = path
        .to_str()
        .filter(|p| absolute(p))
        .ok_or_else(|| error("invalid_read_policy"))?;
    let file = open_at(libc::AT_FDCWD, text, libc::O_RDONLY, false)
        .map_err(|_| error("read_policy_unavailable"))?;
    trusted(&file, 65536, false)?;
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| error("invalid_read_policy"))?;
    if bytes.len() > 65536 {
        return Err(error("invalid_read_policy"));
    }
    let value = parse_bounded_json(&bytes, 8).map_err(|_| error("invalid_read_policy"))?;
    if !value.is_object()
        || !value["executables"].is_object()
        || ["git", "rg", "bwrap"]
            .iter()
            .any(|k| !value["executables"][k].is_object())
        || !value["roots"]
            .as_array()
            .is_some_and(|roots| roots.iter().all(Value::is_object))
    {
        return Err(error("invalid_read_policy"));
    }
    let policy: Policy = serde_json::from_value(value).map_err(|_| error("invalid_read_policy"))?;
    if policy.version != 1
        || policy.profile != "linux-read-v1"
        || !(1..=8).contains(&policy.roots.len())
    {
        return Err(error("invalid_read_policy"));
    }
    let mut ids = std::collections::HashSet::new();
    for root in &policy.roots {
        if !absolute(&root.path)
            || root.id.is_empty()
            || root.id.len() > 64
            || !root.id.as_bytes()[0].is_ascii_lowercase()
            || !root
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
            || !ids.insert(&root.id)
            || path.starts_with(&root.path)
        {
            return Err(error("invalid_read_policy"));
        }
    }
    for (i, root) in policy.roots.iter().enumerate() {
        if policy
            .roots
            .iter()
            .enumerate()
            .any(|(j, other)| i != j && Path::new(&root.path).starts_with(&other.path))
        {
            return Err(error("invalid_read_policy"));
        }
    }
    for binding in [
        &policy.executables.git,
        &policy.executables.rg,
        &policy.executables.bwrap,
    ] {
        if !absolute(&binding.path)
            || !binding.sha256.strip_prefix("sha256:").is_some_and(|v| {
                v.len() == 64
                    && v.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            || policy
                .roots
                .iter()
                .any(|root| Path::new(&binding.path).starts_with(&root.path))
        {
            return Err(error("invalid_read_policy"));
        }
    }
    Ok((policy, digest(&bytes)))
}

pub(super) struct Pinned {
    pub file: File,
    pub identity: Value,
}
pub(super) fn pin(
    path: &str,
    expected: Option<&str>,
    control: &RunControl,
    deadline: Instant,
) -> Result<Pinned, Problem> {
    check(control, deadline)?;
    let file = open_at(libc::AT_FDCWD, path, libc::O_RDONLY, false)
        .map_err(|_| error("read_binding_unavailable"))?;
    pin_file(file, path, expected, control, deadline)
}

pub(super) fn pin_file(
    file: File,
    path: &str,
    expected: Option<&str>,
    control: &RunControl,
    deadline: Instant,
) -> Result<Pinned, Problem> {
    trusted(&file, MAX_FILE, expected.is_some())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 65536];
    let mut count = 0;
    loop {
        check(control, deadline)?;
        let read = file
            .read_at(&mut buffer, count)
            .map_err(|_| error("read_binding_unavailable"))?;
        if read == 0 {
            break;
        }
        if count == 0 && (read < 4 || &buffer[..4] != b"\x7fELF") {
            return Err(error("invalid_read_executable"));
        }
        count += read as u64;
        if count > MAX_FILE {
            return Err(error("invalid_read_executable"));
        }
        hasher.update(&buffer[..read]);
    }
    if count < 4 {
        return Err(error("invalid_read_executable"));
    }
    let sha256 = encoded_digest(&hasher.finalize());
    if expected.is_some_and(|e| sha256 != e) {
        return Err(error("read_digest_mismatch"));
    }
    let meta = file
        .metadata()
        .map_err(|_| error("read_binding_unavailable"))?;
    Ok(Pinned {
        file,
        identity: json!({"path":path,"sha256":sha256,"device":meta.dev(),"inode":meta.ino()}),
    })
}

pub(super) fn repository(root: &File) -> Result<File, Problem> {
    let unsupported = || {
        Problem::unsupported(
            "unsupported_repository",
            "linux-read-v1 supports only ordinary non-bare SHA-1 working trees without sparse/partial/shallow layout, linked worktrees or object alternates.",
        )
    };
    let git = open_at(
        root.as_raw_fd(),
        ".git",
        libc::O_PATH | libc::O_DIRECTORY,
        true,
    )
    .map_err(|_| unsupported())?;
    for name in [
        "commondir",
        "gitdir",
        "shallow",
        "config.worktree",
        "info/sparse-checkout",
        "objects/info/alternates",
        "objects/info/http-alternates",
    ] {
        match open_at(git.as_raw_fd(), name, libc::O_PATH, true) {
            Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {}
            _ => return Err(unsupported()),
        }
    }
    for name in ["objects", "refs"] {
        open_at(
            git.as_raw_fd(),
            name,
            libc::O_PATH | libc::O_DIRECTORY,
            true,
        )
        .map_err(|_| unsupported())?;
    }
    let mut config =
        open_at(git.as_raw_fd(), "config", libc::O_RDONLY, true).map_err(|_| unsupported())?;
    if !config
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= 65536)
    {
        return Err(unsupported());
    }
    let mut text = String::new();
    (&mut config)
        .take(65537)
        .read_to_string(&mut text)
        .map_err(|_| unsupported())?;
    if text.len() > 65536 {
        return Err(unsupported());
    }
    let mut section = String::new();
    let mut format_zero = false;
    for line in text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with(['#', ';']))
    {
        if line.ends_with('\\') {
            return Err(unsupported());
        }
        if let Some(header) = line.strip_prefix('[') {
            let header = header.strip_suffix(']').ok_or_else(unsupported)?;
            section = header
                .split([' ', '\t', '.'])
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if section == "extensions" {
                return Err(unsupported());
            }
            continue;
        }
        let (key, value) = line.split_once('=').unwrap_or((line, "true"));
        let key = key.trim().to_ascii_lowercase();
        let value = value
            .split(['#', ';'])
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"')
            .to_ascii_lowercase();
        if section == "core" {
            match key.as_str() {
                "repositoryformatversion" => {
                    if value != "0" {
                        return Err(unsupported());
                    }
                    format_zero = true;
                }
                "bare" if !matches!(value.as_str(), "false" | "no" | "off" | "0") => {
                    return Err(unsupported());
                }
                "worktree" | "sparsecheckout" | "sparsecheckoutcone" => return Err(unsupported()),
                _ => {}
            }
        }
        if matches!(key.as_str(), "promisor" | "partialclonefilter") {
            return Err(unsupported());
        }
    }
    if !format_zero {
        return Err(unsupported());
    }
    let pack = open_at(
        git.as_raw_fd(),
        "objects/pack",
        libc::O_PATH | libc::O_DIRECTORY,
        true,
    )
    .map_err(|_| unsupported())?;
    let entries = std::fs::read_dir(format!("/proc/self/fd/{}", pack.as_raw_fd()))
        .map_err(|_| unsupported())?;
    for (index, entry) in entries.enumerate() {
        if index > 4096
            || entry
                .map_err(|_| unsupported())?
                .file_name()
                .to_string_lossy()
                .ends_with(".promisor")
        {
            return Err(unsupported());
        }
    }
    Ok(git)
}
