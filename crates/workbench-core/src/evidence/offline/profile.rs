//! A thread-local syscall boundary for the fixed, non-extensible analyzer.
//! No TSYNC: the caller retains its normal authority after the worker joins.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn program() -> Vec<libc::sock_filter> {
    const LOAD: u16 = 0x20;
    const EQUAL: u16 = 0x15;
    const BITS: u16 = 0x45;
    const RETURN: u16 = 0x06;
    const DENY: u32 = 0x0005_0000 | libc::EPERM as u32;
    let mut result = Vec::new();
    let mut emit = |code, jt, jf, k| result.push(libc::sock_filter { code, jt, jf, k });
    emit(LOAD, 0, 0, 4);
    emit(EQUAL, 1, 0, 0xc000_003e); // AUDIT_ARCH_X86_64
    emit(RETURN, 0, 0, DENY); // refuse compat/i386, including socketcall
    emit(LOAD, 0, 0, 0);
    emit(BITS, 0, 1, 0x4000_0000); // refuse x32
    emit(RETURN, 0, 0, DENY);
    for call in [
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_sendto,
        libc::SYS_sendmsg,
        libc::SYS_sendmmsg,
        libc::SYS_recvfrom,
        libc::SYS_recvmsg,
        libc::SYS_recvmmsg,
        libc::SYS_shutdown,
        libc::SYS_getsockname,
        libc::SYS_getpeername,
        libc::SYS_setsockopt,
        libc::SYS_getsockopt,
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_fork,
        libc::SYS_vfork,
        libc::SYS_clone,
        libc::SYS_clone3,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        libc::SYS_ptrace,
        libc::SYS_process_vm_writev,
        libc::SYS_pidfd_getfd,
        libc::SYS_setns,
        libc::SYS_unshare,
    ] {
        emit(EQUAL, 0, 1, call as u32);
        emit(RETURN, 0, 0, DENY);
    }
    emit(RETURN, 0, 0, 0x7fff_0000);
    result
}

pub(in crate::evidence) fn install() -> Result<(), &'static str> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let mut instructions = program();
        let program = libc::sock_fprog {
            len: instructions.len() as u16,
            filter: instructions.as_mut_ptr(),
        };
        // SAFETY: prctl has scalar arguments; the filter is initialized and borrowed
        // for the synchronous seccomp syscall, which copies it into the kernel.
        if unsafe {
            libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                || libc::syscall(libc::SYS_seccomp, 1, 0, &program) != 0
        } {
            return Err("offline_profile_unavailable");
        }
        Ok(())
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    Err("unsupported_platform")
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
mod tests {
    use super::*;

    #[test]
    fn kernel_refuses_network_commands_and_alternate_abi_only_on_worker() {
        // No bind/listen: check socket creation only, then close it immediately.
        let outside = || {
            // SAFETY: socket accepts scalar constants and owns no borrowed memory.
            let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
            assert!(
                fd >= 0,
                "caller socket: {}",
                std::io::Error::last_os_error()
            );
            // SAFETY: fd is the just-created owned descriptor.
            assert_eq!(unsafe { libc::close(fd) }, 0);
        };
        outside();
        std::thread::spawn(|| {
            install().expect("kernel offline profile");
            for call in [
                libc::SYS_socket,
                libc::SYS_connect,
                libc::SYS_sendto,
                libc::SYS_sendmsg,
                libc::SYS_recvfrom,
                libc::SYS_execve,
                libc::SYS_execveat,
                libc::SYS_clone,
                libc::SYS_clone3,
                libc::SYS_fork,
                libc::SYS_vfork,
                libc::SYS_io_uring_setup,
                libc::SYS_io_uring_enter,
                libc::SYS_io_uring_register,
                libc::SYS_socket | 0x4000_0000,
            ] {
                // SAFETY: all tested syscalls are unconditionally refused before
                // the kernel interprets their zero/invalid arguments.
                assert_eq!(unsafe { libc::syscall(call, -1, 0, 0, 0, 0, 0) }, -1);
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EPERM)
                );
            }
        })
        .join()
        .unwrap();
        outside();
    }
}
