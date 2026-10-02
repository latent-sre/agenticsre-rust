//! Fixed internal stage of linux-read-v1, implemented by the hosting CLI image.
use std::{fs::File, path::PathBuf};

pub(crate) const ENTRY: &str = "__save_linux_read_v1_launch";
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
// Internal descriptor payload only. The public call/policy cap stays 64 KiB;
// CLI arguments can expand to six JSON bytes per admitted control character.
pub(crate) const MAX_METADATA_BYTES: usize = 256 * 1024;

/// Trusted embedding context. The only public constructor opens this running image;
/// command requests and policy files cannot supply a replacement launcher path.
pub struct ReadProfileLauncher {
    pub(crate) image: File,
    pub(crate) path: PathBuf,
}
impl ReadProfileLauncher {
    pub fn current() -> std::io::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            Ok(Self {
                image: File::open("/proc/self/exe")?,
                path: std::env::current_exe()?,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "read launcher requires Linux",
            ))
        }
    }
}

/// Call before ordinary CLI parsing when embedding the current-image launcher.
/// Direct invocations without the private descriptor and namespace contract fail.
pub fn read_launcher_entry() -> Option<u8> {
    if std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new(ENTRY)) {
        return None;
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Some(linux::entry())
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        Some(125)
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux {
    use super::MAX_METADATA_BYTES;
    use crate::{
        process,
        read_profile::{grammar, seccomp},
        request::parse_bounded_json,
    };
    use serde::Deserialize;
    use serde_json::{Value, json};
    use std::{
        fs::{File, OpenOptions},
        io::{Read, Write},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{
                fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
                process::ExitStatusExt,
            },
        },
        process::{Command, Stdio},
    };

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Metadata {
        version: u64,
        input: Value,
        root_device: u64,
        root_inode: u64,
        tool_device: u64,
        tool_inode: u64,
    }
    fn send(file: &mut File, record: Value) -> Result<(), ()> {
        let mut bytes = serde_json::to_vec(&record).map_err(|_| ())?;
        bytes.push(b'\n');
        if bytes.len() > 256 {
            return Err(());
        }
        file.write_all(&bytes).map_err(|_| ())
    }
    fn descriptor(value: &std::ffi::OsStr) -> Option<i32> {
        value.to_str()?.parse::<i32>().ok().filter(|fd| *fd > 2)
    }
    fn channel(fd: i32) -> Result<File, ()> {
        // SAFETY: F_GETFL only examines descriptor state, before ownership is taken.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || flags & libc::O_ACCMODE != libc::O_WRONLY || flags & libc::O_NONBLOCK == 0 {
            return Err(());
        }
        // SAFETY: this fixed child entry takes ownership of the explicitly passed fd.
        let file = unsafe { File::from_raw_fd(fd) };
        let metadata = file.metadata().map_err(|_| ())?;
        if !metadata.file_type().is_fifo() {
            return Err(());
        }
        for standard in 0..=2 {
            // SAFETY: stat is initialized storage for fstat's output.
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            // SAFETY: the output pointer is valid; fstat does not retain it.
            if unsafe { libc::fstat(standard, &mut stat) } != 0
                || (stat.st_dev == metadata.dev() && stat.st_ino == metadata.ino())
            {
                return Err(());
            }
        }
        // All private descriptors close during tool exec. This is also repeated by
        // Command's normal CLOEXEC handling; the payload never receives this writer.
        // SAFETY: close_range changes fd flags in this disposable launcher only.
        if unsafe {
            libc::syscall(
                libc::SYS_close_range,
                3_u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            )
        } != 0
        {
            return Err(());
        }
        Ok(file)
    }
    fn decode_metadata(bytes: &[u8]) -> Result<Metadata, ()> {
        if bytes.len() > MAX_METADATA_BYTES {
            return Err(());
        }
        let value = parse_bounded_json(bytes, 8).map_err(|_| ())?;
        if !value.is_object() || !value["input"].is_object() {
            return Err(());
        }
        let metadata: Metadata = serde_json::from_value(value).map_err(|_| ())?;
        if metadata.version != 1 {
            return Err(());
        }
        Ok(metadata)
    }
    fn admit(fd: i32) -> Result<grammar::Command, ()> {
        // SAFETY: the private metadata descriptor is distinct from channel/stdio.
        let file = unsafe { File::from_raw_fd(fd) };
        let seal = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
        // SAFETY: F_GET_SEALS reads flags from this live descriptor.
        let seals = unsafe { libc::fcntl(fd, libc::F_GET_SEALS) };
        if seals < 0
            || seals & seal != seal
            || !file
                .metadata()
                .is_ok_and(|m| m.is_file() && m.len() <= MAX_METADATA_BYTES as u64)
        {
            return Err(());
        }
        let mut bytes = Vec::new();
        file.take(MAX_METADATA_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ())?;
        let metadata = decode_metadata(&bytes)?;
        // Ordinary direct CLI invocation cannot satisfy this fixed namespace shape.
        // These checks supplement admission; they do not authenticate a same-OS caller.
        // SAFETY: getpid/prctl read process-local scalar state.
        let (pid, no_new_privileges) = unsafe {
            (
                libc::getpid(),
                libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0),
            )
        };
        if pid != 2 || no_new_privileges != 1 || std::path::Path::new("/proc").exists() {
            return Err(());
        }
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_NOFOLLOW)
            .open("/workspace")
            .map_err(|_| ())?;
        let r = root.metadata().map_err(|_| ())?;
        if !r.is_dir() || r.dev() != metadata.root_device || r.ino() != metadata.root_inode {
            return Err(());
        }
        // SAFETY: stat is valid initialized output storage for fstatvfs.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: root is a live descriptor and the output pointer remains valid.
        if unsafe { libc::fstatvfs(root.as_raw_fd(), &mut stat) } != 0
            || stat.f_flag & libc::ST_RDONLY == 0
        {
            return Err(());
        }
        let tool = std::fs::symlink_metadata("/tool").map_err(|_| ())?;
        if !tool.is_file()
            || tool.dev() != metadata.tool_device
            || tool.ino() != metadata.tool_inode
        {
            return Err(());
        }
        let input = process::validate_input(&metadata.input).map_err(|_| ())?;
        grammar::normalize(&input).map_err(|_| ())
    }
    fn apply_filter() -> Result<(), ()> {
        let bytes = seccomp::program();
        let mut filters: Vec<libc::sock_filter> = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| libc::sock_filter {
                code: u16::from_le_bytes(b[..2].try_into().expect("instruction size")),
                jt: b[2],
                jf: b[3],
                k: u32::from_le_bytes(b[4..].try_into().expect("instruction size")),
            })
            .collect();
        let program = libc::sock_fprog {
            len: filters.len().try_into().map_err(|_| ())?,
            filter: filters.as_mut_ptr(),
        };
        // SAFETY: the BPF slice and program remain alive while prctl copies them.
        if unsafe { libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &program) } != 0 {
            return Err(());
        }
        Ok(())
    }
    fn landlock() -> Result<u64, ()> {
        // SAFETY: the version query has no pointers and changes no policy.
        let abi = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<u8>(),
                0_usize,
                1_u32,
            )
        };
        if abi < 5 {
            return Err(());
        }
        let null = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_NOFOLLOW)
            .open("/dev/null")
            .map_err(|_| ())?;
        let metadata = null.metadata().map_err(|_| ())?;
        if !metadata.file_type().is_char_device()
            || libc::major(metadata.rdev()) != 1
            || libc::minor(metadata.rdev()) != 3
        {
            return Err(());
        }
        // ABI5: WRITE_FILE plus all mutation rights REMOVE_DIR through IOCTL_DEV.
        // READ_FILE/READ_DIR/EXECUTE remain governed by the namespace and pinned mounts.
        let rights: u64 = (1 << 1) | ((1 << 16) - (1 << 4));
        // SAFETY: the ABI accepts an eight-byte handled_access_fs ruleset prefix.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                &rights,
                std::mem::size_of_val(&rights),
                0_u32,
            )
        } as i32;
        if fd < 0 {
            return Err(());
        }
        // SAFETY: create_ruleset returned this new owned descriptor.
        let rules = unsafe { File::from_raw_fd(fd) };
        #[repr(C, packed)]
        struct Beneath {
            allowed: u64,
            parent: i32,
        }
        let beneath = Beneath {
            allowed: 1 << 1,
            parent: null.as_raw_fd(),
        };
        // SAFETY: the packed ABI structure remains valid for the synchronous syscall.
        if unsafe { libc::syscall(libc::SYS_landlock_add_rule, fd, 1_u32, &beneath, 0_u32) } != 0 {
            return Err(());
        }
        // SAFETY: restrict_self applies these already prepared rules only to this
        // single-threaded launcher and its future descendants; no host policy changes.
        if unsafe { libc::syscall(libc::SYS_landlock_restrict_self, rules.as_raw_fd(), 0_u32) } != 0
        {
            return Err(());
        }
        Ok(abi as u64)
    }
    pub(super) fn entry() -> u8 {
        let args: Vec<_> = std::env::args_os().collect();
        if args.len() != 4 {
            return 125;
        }
        let (Some(meta), Some(report)) = (descriptor(&args[2]), descriptor(&args[3])) else {
            return 125;
        };
        if meta == report {
            return 125;
        }
        let Ok(mut status) = channel(report) else {
            return 125;
        };
        let command = match admit(meta) {
            Ok(command) => command,
            Err(()) => {
                let _ = send(
                    &mut status,
                    json!({"stage":"setup_failed","code":"launcher_admission_failed"}),
                );
                return 125;
            }
        };
        let abi = match apply_filter().and_then(|_| landlock()) {
            Ok(abi) => abi,
            Err(()) => {
                let _ = send(
                    &mut status,
                    json!({"stage":"setup_failed","code":"landlock_unavailable"}),
                );
                return 125;
            }
        };
        if send(&mut status, json!({"stage":"admitted","abi":abi})).is_err() {
            return 125;
        }
        let mut child = match Command::new("/tool")
            .args(command.args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => {
                let _ = send(
                    &mut status,
                    json!({"stage":"exec_failed","code":"tool_exec_failed"}),
                );
                return 125;
            }
        };
        if send(&mut status, json!({"stage":"tool_started"})).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return 125;
        }
        let exit = match child.wait() {
            Ok(exit) => exit,
            Err(_) => return 125,
        };
        let code = exit
            .code()
            .unwrap_or_else(|| 128 + exit.signal().unwrap_or(0));
        if send(&mut status, json!({"stage":"tool_exit","code":code})).is_err() {
            return 125;
        }
        code as u8
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn maximum_admitted_escaped_arguments_fit_private_metadata() {
            // Five 204-byte components give the full 1024-byte relative path
            // bound without exceeding a Linux filename's 255-byte component cap.
            // Every nonseparator byte needs the longest JSON escape (six bytes).
            let path = vec!["\u{1}".repeat(204); 5].join("/");
            assert_eq!(path.len(), 1024);
            let cwd = format!("/{}", &path[..1023]);
            let mut args = vec![
                "-n".to_owned(),
                "-i".into(),
                "-F".into(),
                "--hidden".into(),
                "-e".into(),
                "\u{1}".repeat(4096),
                "--".into(),
            ];
            args.extend(std::iter::repeat_n(path, 32));
            let input = json!({"program":"rg", "cwd":cwd, "args":args});
            let parsed = process::validate_input(&input).unwrap();
            assert!(grammar::normalize(&parsed).is_ok());
            let bytes = serde_json::to_vec(&json!({"version":1,"input":input,"root_device":u64::MAX,"root_inode":u64::MAX,"tool_device":u64::MAX,"tool_inode":u64::MAX})).unwrap();
            assert!(
                bytes.len() > 65536,
                "fixture must cross the old private cap"
            );
            let decoded = decode_metadata(&bytes).unwrap_or_else(|_| {
                panic!(
                    "valid maximum escaped arguments rejected at {} encoded bytes",
                    bytes.len()
                )
            });
            assert_eq!(decoded.input, input);
            assert!(decode_metadata(&vec![b' '; MAX_METADATA_BYTES + 1]).is_err());
        }
    }
}
