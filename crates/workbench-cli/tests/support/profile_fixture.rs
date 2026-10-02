//! Test-only trusted policy binding used to exercise the boundary below Git/rg.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn main() {
    use serde_json::{Value, json};
    use std::{
        fs,
        io::Write,
        net::{SocketAddr, TcpStream},
        os::unix::{fs::OpenOptionsExt, net::UnixStream},
        time::Duration,
    };
    let config: Value =
        serde_json::from_slice(&fs::read("/workspace/mode.json").expect("mode file"))
            .expect("mode JSON");
    match config["mode"].as_str().expect("mode") {
        "boundary" => {
            let tcp: SocketAddr = format!("127.0.0.1:{}", config["port"].as_u64().unwrap())
                .parse()
                .unwrap();
            let write_errno = fs::write("/workspace/write-canary", b"forbidden")
                .unwrap_err()
                .raw_os_error();
            let fifo_errno = fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open("/workspace/canary.fifo")
                .unwrap_err()
                .raw_os_error();
            let fifo_readwrite_errno = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open("/workspace/canary.fifo")
                .unwrap_err()
                .raw_os_error();
            // Exercise legacy open directly as well as libc's normal openat path.
            // SAFETY: both paths are fixed NUL-terminated namespace-local fixtures.
            let legacy_fifo = unsafe {
                libc::syscall(
                    libc::SYS_open,
                    c"/workspace/canary.fifo".as_ptr(),
                    libc::O_WRONLY | libc::O_NONBLOCK,
                    0,
                )
            };
            assert_eq!(legacy_fifo, -1);
            let legacy_fifo_errno = std::io::Error::last_os_error().raw_os_error();
            // SAFETY: open has a valid constant pathname and no output pointer.
            let readonly_truncate = unsafe {
                libc::syscall(
                    libc::SYS_open,
                    c"/workspace/read.txt".as_ptr(),
                    libc::O_RDONLY | libc::O_TRUNC,
                    0,
                )
            };
            assert_eq!(readonly_truncate, -1);
            let readonly_truncate_errno = std::io::Error::last_os_error().raw_os_error();
            let file_write_errno = fs::OpenOptions::new()
                .write(true)
                .open("/workspace/read.txt")
                .unwrap_err()
                .raw_os_error();
            let device_errno = fs::OpenOptions::new()
                .write(true)
                .open("/dev/zero")
                .unwrap_err()
                .raw_os_error();
            let mut null = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/null")
                .unwrap();
            let null_write = null.write_all(b"private sink").is_ok();
            drop(null);
            let mut unexpected_fds = Vec::new();
            for fd in 3..1024 {
                // SAFETY: F_GETFD inspects an integer fd without accessing Rust memory.
                if unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0 {
                    unexpected_fds.push(fd);
                }
            }
            // SAFETY: prctl reads the current process's scalar privilege flag.
            let no_new_privileges = unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) };
            let report = json!({"read":fs::read_to_string("/workspace/read.txt").unwrap(),
                "outside_denied":fs::read("/workspace/outside-link").is_err(),"proc_absent":!std::path::Path::new("/proc").exists(),
                "write_errno":write_errno,"fifo_errno":fifo_errno,"fifo_readwrite_errno":fifo_readwrite_errno,"legacy_fifo_errno":legacy_fifo_errno,"readonly_truncate_errno":readonly_truncate_errno,"file_write_errno":file_write_errno,"device_errno":device_errno,"null_write":null_write,"tcp_errno":TcpStream::connect_timeout(&tcp,Duration::from_millis(100)).unwrap_err().raw_os_error(),
                "unix_errno":UnixStream::connect("/workspace/canary.sock").unwrap_err().raw_os_error(),
                "unexpected_fds":unexpected_fds,"no_new_privileges":no_new_privileges,
                "ambient_secret":std::env::var("PROFILE_CANARY_SECRET").ok()});
            println!("{report}");
        }
        "flood" => {
            for _ in 0..1024 {
                std::io::stdout().write_all(&[b'\0'; 4096]).unwrap();
            }
        }
        "sleep" => {
            // SAFETY: SIG_IGN is a valid signal disposition; only this fixture is affected.
            unsafe {
                libc::signal(libc::SIGTERM, libc::SIG_IGN);
            }
            println!("ready");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        "detached" => {
            // SAFETY: the fixture is single-threaded before fork and the child uses
            // only scalar libc calls, then pauses until namespace teardown.
            let child = unsafe { libc::fork() };
            assert!(child >= 0);
            if child == 0 {
                let name = std::ffi::CString::new(config["name"].as_str().unwrap()).unwrap();
                // SAFETY: setsid/signal/prctl operate on this disposable child; the
                // name pointer remains valid until prctl returns.
                unsafe {
                    libc::setsid();
                    libc::signal(libc::SIGTERM, libc::SIG_IGN);
                    libc::prctl(libc::PR_SET_NAME, name.as_ptr(), 0, 0, 0);
                    loop {
                        libc::pause();
                    }
                }
            }
            println!("detached child {child}");
            std::thread::sleep(Duration::from_millis(50));
        }
        "kill-launcher" => {
            // SAFETY: this is the intentionally adversarial disposable payload's
            // parent inside its private PID namespace, never a host PID.
            unsafe {
                libc::kill(libc::getppid(), libc::SIGKILL);
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        "x32" => {
            // SAFETY: getpid has no pointer arguments. The required filter kills
            // this disposable process before interpreting an alternate ABI.
            unsafe {
                libc::syscall(0x4000_0000 | libc::SYS_getpid);
            }
            panic!("x32 syscall was not rejected");
        }
        "i386" => {
            // SAFETY: i386 getpid (20) has no pointer arguments; the test expects
            // process death at the seccomp architecture gate.
            unsafe {
                std::arch::asm!("int 0x80", inlateout("eax") 20_u32 => _, options(nostack));
            }
            panic!("i386 syscall was not rejected");
        }
        other => panic!("unknown fixture mode {other}"),
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn main() {
    panic!("profile fixture requires x86_64 Linux");
}
