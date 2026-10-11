//! One explicitly identified Linux process, observed through kernel procfs only.
use super::{Outcome, config::Process};
use serde_json::json;

#[cfg(target_os = "linux")]
pub(super) fn vantage() -> serde_json::Value {
    use std::os::unix::fs::MetadataExt;
    let namespaces = ["net", "pid", "user", "mnt"].into_iter().map(|kind| {
        let identity = std::fs::metadata(format!("/proc/self/ns/{kind}")).ok()
            .map(|metadata| json!({"device":metadata.dev().to_string(),"inode":metadata.ino().to_string()}));
        (kind.to_owned(), json!(identity))
    }).collect::<serde_json::Map<_, _>>();
    // SAFETY: getuid/geteuid only return the current process's numerical identity.
    let (uid, effective_uid) = unsafe { (libc::getuid(), libc::geteuid()) };
    json!({"platform":"linux","observer_pid":std::process::id(),"uid":uid,"effective_uid":effective_uid,"namespaces":namespaces})
}

#[cfg(not(target_os = "linux"))]
pub(super) fn vantage() -> serde_json::Value {
    json!({"platform":std::env::consts::OS,"observer_pid":std::process::id(),"namespaces":{}})
}

#[cfg(target_os = "linux")]
pub(super) async fn observe(binding: Option<&Process>) -> Outcome {
    use std::{
        ffi::CString,
        fs::{File, OpenOptions},
        io::Read,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::OpenOptionsExt,
        },
    };
    let Some(binding) = binding else {
        return Outcome::omitted("unconfigured", "process_unconfigured");
    };
    if binding.backend != "linux-process-v1" {
        return Outcome::omitted("unsupported", "process_backend_unsupported");
    }
    fn read(dir: &File, name: &str) -> Result<String, &'static str> {
        let name = CString::new(name).expect("fixed proc metadata name");
        // SAFETY: dir is a live descriptor and name is a fixed one-component filename.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err("process_metadata_unavailable");
        }
        // SAFETY: successful openat returned a uniquely owned descriptor.
        let mut file = unsafe { File::from_raw_fd(fd) };
        // SAFETY: the initialized statfs buffer is valid for this owned metadata descriptor.
        let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: fstatfs borrows one live fd and writes exactly one statfs structure.
        if unsafe { libc::fstatfs(file.as_raw_fd(), &mut filesystem) } != 0
            || filesystem.f_type as u64 != 0x9fa0
            || !file.metadata().is_ok_and(|metadata| metadata.is_file())
        {
            return Err("process_prerequisite_unavailable");
        }
        let mut bytes = Vec::new();
        file.by_ref()
            .take(8193)
            .read_to_end(&mut bytes)
            .map_err(|_| "process_metadata_unavailable")?;
        if bytes.len() > 8192 {
            return Err("process_metadata_invalid");
        }
        String::from_utf8(bytes).map_err(|_| "process_metadata_invalid")
    }
    fn snapshot(dir: &File, pid: u32) -> Result<(char, String, [u32; 4]), &'static str> {
        let stat = read(dir, "stat")?;
        let (prefix, fields) = stat.rsplit_once(')').ok_or("process_metadata_invalid")?;
        if !prefix.starts_with(&format!("{pid} (")) {
            return Err("process_metadata_invalid");
        }
        let fields: Vec<_> = fields.split_ascii_whitespace().collect();
        if fields.len() < 20 || fields[0].len() != 1 {
            return Err("process_metadata_invalid");
        }
        let ticks = fields[19]
            .parse::<u64>()
            .map_err(|_| "process_metadata_invalid")?
            .to_string();
        let status = read(dir, "status")?;
        let mut uid = status.lines().filter_map(|line| line.strip_prefix("Uid:"));
        let value = uid.next().ok_or("process_metadata_invalid")?;
        if uid.next().is_some() {
            return Err("process_metadata_invalid");
        }
        let ids = value
            .split_ascii_whitespace()
            .map(str::parse::<u32>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "process_metadata_invalid")?;
        let ids: [u32; 4] = ids.try_into().map_err(|_| "process_metadata_invalid")?;
        Ok((
            fields[0].chars().next().expect("one ASCII state"),
            ticks,
            ids,
        ))
    }
    let procfs = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/proc")
    {
        Ok(file) => file,
        Err(_) => {
            return Outcome::failed(
                "process_prerequisite_unavailable",
                json!({"state":"unavailable"}),
            );
        }
    };
    // SAFETY: fstatfs writes one initialized integer-only structure for the owned fd.
    let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: procfs remains open and filesystem is valid writable storage.
    if unsafe { libc::fstatfs(procfs.as_raw_fd(), &mut filesystem) } != 0
        || filesystem.f_type as u64 != 0x9fa0
    {
        return Outcome::failed(
            "process_prerequisite_unavailable",
            json!({"state":"unavailable"}),
        );
    }
    let name = CString::new(binding.pid.to_string()).expect("numeric PID");
    // SAFETY: the fixed numeric component is resolved relative to the confirmed procfs directory.
    let fd = unsafe {
        libc::openat(
            procfs.as_raw_fd(),
            name.as_ptr(),
            libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let code = std::io::Error::last_os_error().raw_os_error();
        return Outcome::failed(
            if code == Some(libc::ENOENT) {
                "process_gone"
            } else {
                "process_metadata_unavailable"
            },
            json!({"state":if code == Some(libc::ENOENT) {"gone"} else {"unavailable"},"pid":binding.pid}),
        );
    }
    // SAFETY: fd is newly owned after successful openat.
    let directory = unsafe { File::from_raw_fd(fd) };
    let observation = (|| {
        let first = snapshot(&directory, binding.pid)?;
        let second = snapshot(&directory, binding.pid)?;
        if first.1 != second.1
            || first.2 != second.2
            || second.1 != binding.start_time_ticks
            || second.2[0] != binding.uid
        {
            return Err("process_identity_mismatch");
        }
        let state = match second.0 {
            'R' => "running",
            'S' | 'D' | 'I' => "sleeping",
            'T' | 't' => "stopped",
            'Z' => "zombie",
            'X' | 'x' => "gone",
            _ => return Err("process_state_unsupported"),
        };
        Ok(
            json!({"backend":"linux-process-v1","pid":binding.pid,"start_time_ticks":second.1,
            "uid":second.2[0],"uids":{"real":second.2[0],"effective":second.2[1],"saved":second.2[2],"filesystem":second.2[3]},
            "state":state,"vantage":vantage(),"identity_verified":true}),
        )
    })();
    match observation {
        Ok(data) if data["state"] == "gone" => Outcome::failed("process_gone", data),
        Ok(data) => Outcome::succeeded(data),
        Err(code) => Outcome::failed(
            code,
            json!({"backend":"linux-process-v1","pid":binding.pid,"state":if code == "process_identity_mismatch" {"identity_mismatch"} else {"unavailable"},"identity_verified":false}),
        ),
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) async fn observe(_binding: Option<&Process>) -> Outcome {
    Outcome::omitted("unsupported", "process_platform_unsupported")
}
