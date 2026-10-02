//! Fixed x86_64 Linux filter. Namespace and mount containment remain separate gates.
// seccomp_data offsets and return values follow linux/seccomp.h; the architecture
// gate also rejects i386 socketcall and x32 variants before syscall interpretation.
const LOAD: u16 = 0x20;
const EQUAL: u16 = 0x15;
const BITS: u16 = 0x45;
const RETURN: u16 = 0x06;
const KILL: u32 = 0x8000_0000;
const DENY: u32 = 0x0005_0000 | libc::EPERM as u32;
const ALLOW: u32 = 0x7fff_0000;

fn emit(program: &mut Vec<u8>, code: u16, yes: u8, no: u8, value: u32) {
    program.extend_from_slice(&code.to_le_bytes());
    program.extend_from_slice(&[yes, no]);
    program.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn program() -> Vec<u8> {
    let mut bpf = Vec::new();
    emit(&mut bpf, LOAD, 0, 0, 4); // audit architecture
    emit(&mut bpf, EQUAL, 1, 0, 0xc000_003e); // AUDIT_ARCH_X86_64
    emit(&mut bpf, RETURN, 0, 0, KILL);
    emit(&mut bpf, LOAD, 0, 0, 0); // syscall number
    emit(&mut bpf, BITS, 0, 1, 0x4000_0000); // __X32_SYSCALL_BIT
    emit(&mut bpf, RETURN, 0, 0, KILL);
    for call in [
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_openat2,
        libc::SYS_creat,
        libc::SYS_open_by_handle_at,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_chroot,
        libc::SYS_move_mount,
        libc::SYS_open_tree,
        libc::SYS_fsopen,
        libc::SYS_fsmount,
        libc::SYS_fspick,
        libc::SYS_fsconfig,
        libc::SYS_mount_setattr,
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_pidfd_getfd,
        libc::SYS_ptrace,
        libc::SYS_process_vm_writev,
    ] {
        emit(&mut bpf, EQUAL, 0, 1, call as u32);
        emit(&mut bpf, RETURN, 0, 0, DENY);
    }
    // clone3 arguments are behind a pointer. ENOSYS permits libc's normal clone
    // fallback, whose namespace flags can be checked directly below.
    emit(&mut bpf, EQUAL, 0, 1, libc::SYS_clone3 as u32);
    emit(&mut bpf, RETURN, 0, 0, 0x0005_0000 | libc::ENOSYS as u32);
    // Landlock mediates WRITE_FILE by inode, admitting only private /dev/null.
    // Creation/truncation remains impossible even on that device through open.
    let writable =
        (libc::O_CREAT | libc::O_TRUNC | libc::O_APPEND | (libc::O_TMPFILE & !libc::O_DIRECTORY))
            as u32;
    for (call, argument, mask) in [
        (libc::SYS_open, 1, writable),
        (libc::SYS_openat, 2, writable),
        (
            libc::SYS_clone,
            0,
            (libc::CLONE_NEWUSER
                | libc::CLONE_NEWNS
                | libc::CLONE_NEWNET
                | libc::CLONE_NEWPID
                | libc::CLONE_NEWIPC
                | libc::CLONE_NEWUTS
                | libc::CLONE_NEWCGROUP) as u32,
        ),
    ] {
        emit(&mut bpf, EQUAL, 0, 3, call as u32);
        emit(&mut bpf, LOAD, 0, 0, 16 + argument * 8); // little-endian low argument word
        emit(&mut bpf, BITS, 0, 1, mask);
        emit(&mut bpf, RETURN, 0, 0, DENY);
        emit(&mut bpf, LOAD, 0, 0, 0);
    }
    emit(&mut bpf, RETURN, 0, 0, ALLOW);
    bpf
}

#[cfg(test)]
mod tests {
    use super::*;
    // Evaluate the small generated instruction subset against seccomp_data. Native
    // tests independently exercise the kernel filter in the required Cally lane.
    fn evaluate(arch: u32, syscall: i64, argument: usize, flags: u32) -> u32 {
        let mut data = [0_u32; 16];
        data[0] = syscall as u32;
        data[1] = arch;
        data[4 + argument * 2] = flags;
        let program = program();
        let mut index = 0;
        let mut accumulator = 0;
        loop {
            let op = &program[index * 8..index * 8 + 8];
            let code = u16::from_le_bytes(op[..2].try_into().unwrap());
            let value = u32::from_le_bytes(op[4..].try_into().unwrap());
            match code {
                LOAD => accumulator = data[value as usize / 4],
                EQUAL => index += usize::from(if accumulator == value { op[2] } else { op[3] }),
                BITS => {
                    index += usize::from(if accumulator & value != 0 {
                        op[2]
                    } else {
                        op[3]
                    })
                }
                RETURN => return value,
                _ => panic!("unexpected filter instruction"),
            }
            index += 1;
        }
    }
    #[test]
    fn filter_distinguishes_read_flags_and_rejects_alternative_abis() {
        for call in [
            libc::SYS_socket,
            libc::SYS_socketpair,
            libc::SYS_openat2,
            libc::SYS_io_uring_setup,
            libc::SYS_mount,
        ] {
            assert_eq!(evaluate(0xc000_003e, call, 0, 0), DENY);
        }
        assert_eq!(evaluate(0x4000_0003, 102, 0, 0), KILL);
        assert_eq!(
            evaluate(0xc000_003e, 0x4000_0000 | libc::SYS_openat, 0, 0),
            KILL
        );
        for (call, arg) in [(libc::SYS_open, 1), (libc::SYS_openat, 2)] {
            assert_eq!(
                evaluate(0xc000_003e, call, arg, libc::O_RDONLY as u32),
                ALLOW
            );
            assert_eq!(
                evaluate(
                    0xc000_003e,
                    call,
                    arg,
                    (libc::O_DIRECTORY | libc::O_CLOEXEC) as u32
                ),
                ALLOW
            );
            for flag in [libc::O_CREAT, libc::O_TRUNC, libc::O_APPEND] {
                assert_eq!(evaluate(0xc000_003e, call, arg, flag as u32), DENY);
            }
            for flag in [libc::O_WRONLY, libc::O_RDWR] {
                assert_eq!(evaluate(0xc000_003e, call, arg, flag as u32), ALLOW);
            }
        }
        assert_eq!(
            evaluate(0xc000_003e, libc::SYS_clone, 0, libc::CLONE_NEWUSER as u32),
            DENY
        );
        assert_eq!(evaluate(0xc000_003e, libc::SYS_write, 0, 0), ALLOW);
    }
}
