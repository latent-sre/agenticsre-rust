//! Final result delivery is also bounded: a live consumer that never reads must not hang us.
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(not(target_os = "linux"))]
pub fn write(bytes: &[u8], _signal: &AtomicUsize) -> io::Result<()> {
    use std::io::Write;
    std::io::stdout().lock().write_all(bytes)
}

#[cfg(target_os = "linux")]
pub fn write(mut bytes: &[u8], signal: &AtomicUsize) -> io::Result<()> {
    use std::time::{Duration, Instant};
    const FD: i32 = libc::STDOUT_FILENO;
    // SAFETY: stdout is a process-owned descriptor; fcntl neither retains pointers nor
    // transfers ownership. Flags are restored on every return by Restore.
    let flags = unsafe { libc::fcntl(FD, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    struct Restore(i32);
    impl Drop for Restore {
        fn drop(&mut self) {
            // SAFETY: restore only changes the original stdout descriptor's status flags.
            unsafe {
                libc::fcntl(FD, libc::F_SETFL, self.0);
            }
        }
    }
    let _restore = Restore(flags);
    // SAFETY: set the nonblocking status flag on a valid stdout descriptor.
    if unsafe { libc::fcntl(FD, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut attempted = false;
    while !bytes.is_empty() {
        // Give an already-cancelled invocation one nonblocking attempt to deliver its
        // receipt. If output cannot complete immediately, a signal ends delivery rather
        // than waiting for the independent consumer timeout. This also observes signals
        // arriving after the primary child and its cleanup have already finished.
        if attempted && signal.load(Ordering::Relaxed) != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "result delivery cancelled",
            ));
        }
        attempted = true;
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "result consumer did not drain",
            ));
        }
        // SAFETY: bytes points to readable memory for exactly bytes.len() bytes; libc
        // copies bytes during the call and does not retain the slice.
        let written = unsafe { libc::write(FD, bytes.as_ptr().cast(), bytes.len()) };
        if written > 0 {
            bytes = &bytes[written as usize..];
            continue;
        }
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "result consumer did not accept bytes",
            ));
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if error.kind() != io::ErrorKind::WouldBlock {
            return Err(error);
        }
        let mut poll = libc::pollfd {
            fd: FD,
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: poll references one initialized element for a bounded call.
        unsafe {
            libc::poll(&mut poll, 1, 20);
        }
    }
    Ok(())
}
