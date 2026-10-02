//! Private launch seam for descriptor-pinned profile bindings. No wire access.
use std::{
    ffi::CString,
    fs::File,
    io,
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::Command,
};

pub(crate) struct DescriptorLaunch {
    pub executable: File,
    pub inherited: Vec<File>,
}

impl DescriptorLaunch {
    pub(super) fn configure(&self, command: &mut Command, args: &[String]) -> io::Result<()> {
        let executable = self.executable.as_raw_fd();
        let fds: Vec<i32> = self.inherited.iter().map(AsRawFd::as_raw_fd).collect();
        let argv: Vec<CString> = std::iter::once("bwrap")
            .chain(args.iter().map(String::as_str))
            .map(CString::new)
            .collect::<Result<_, _>>()
            .map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
        if argv.len() >= 512 {
            return Err(io::Error::from_raw_os_error(libc::E2BIG));
        }
        let env: Vec<CString> = super::CHILD_ENV
            .iter()
            .map(|(k, v)| CString::new(format!("{k}={v}")).expect("fixed environment"))
            .collect();
        // SAFETY: the callback uses only fixed stack arrays and async-signal-safe
        // syscalls after fork. PreparedProcess retains every referenced descriptor
        // through spawn. execveat uses this pinned ELF fd, never its pathname.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                // Mark all ambient descriptors for closure, including caller-owned
                // non-CLOEXEC fds. Clear CLOEXEC only for the wrapper handoff set.
                if libc::syscall(
                    libc::SYS_close_range,
                    3_u32,
                    u32::MAX,
                    libc::CLOSE_RANGE_CLOEXEC,
                ) != 0
                {
                    return Err(io::Error::last_os_error());
                }
                for fd in &fds {
                    if libc::fcntl(*fd, libc::F_SETFD, 0) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                }
                let mut argument_pointers = [std::ptr::null(); 512];
                for (slot, arg) in argument_pointers.iter_mut().zip(&argv) {
                    *slot = arg.as_ptr();
                }
                let mut environment_pointers = [std::ptr::null(); 5];
                for (slot, var) in environment_pointers.iter_mut().zip(&env) {
                    *slot = var.as_ptr();
                }
                libc::syscall(
                    libc::SYS_execveat,
                    executable,
                    c"".as_ptr(),
                    argument_pointers.as_ptr(),
                    environment_pointers.as_ptr(),
                    libc::AT_EMPTY_PATH,
                );
                Err(io::Error::last_os_error())
            });
        }
        Ok(())
    }
}
