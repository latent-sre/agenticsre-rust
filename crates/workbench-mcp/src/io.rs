//! Linux pipe ownership. Reopen fixed proc descriptors; never mutate inherited status flags.
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    os::fd::{AsRawFd, RawFd},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
};

pub(crate) fn pipe(fd: RawFd, read: bool) -> io::Result<File> {
    // SAFETY: fcntl reads flags of the supplied standard descriptor, without changing them.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || flags & libc::O_ACCMODE != if read { libc::O_RDONLY } else { libc::O_WRONLY } {
        return Err(io::Error::other("unsupported pipe access"));
    }
    let mut before = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstat initializes valid storage and retains descriptor ownership.
    if unsafe { libc::fstat(fd, before.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstat initialized the complete value.
    let before = unsafe { before.assume_init() };
    if before.st_mode & libc::S_IFMT != libc::S_IFIFO {
        return Err(io::Error::other("stdio requires pipes or FIFOs"));
    }
    let file = OpenOptions::new()
        .read(read)
        .write(!read)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(format!("/proc/self/fd/{fd}"))?;
    let after = file.metadata()?;
    if after.dev() != before.st_dev
        || after.ino() != before.st_ino
        || after.mode() & libc::S_IFMT != libc::S_IFIFO
    {
        return Err(io::Error::other("stdio identity changed"));
    }
    Ok(file)
}

pub(crate) fn diagnostics() -> Option<File> {
    if let Ok(file) = pipe(2, false) {
        return Some(file);
    }
    let mut before = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstat initializes valid storage without opening an unsupported diagnostic sink.
    if unsafe { libc::fstat(2, before.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: successful fstat initialized the value.
    let before = unsafe { before.assume_init() };
    if before.st_mode & libc::S_IFMT != libc::S_IFCHR || before.st_rdev != libc::makedev(1, 3) {
        return None;
    }
    let file = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open("/proc/self/fd/2")
        .ok()?;
    let meta = file.metadata().ok()?;
    // Only the verified Linux null character device is accepted besides a pipe.
    (meta.dev() == before.st_dev
        && meta.ino() == before.st_ino
        && meta.mode() & libc::S_IFMT == libc::S_IFCHR
        && meta.rdev() == libc::makedev(1, 3))
    .then_some(file)
}

pub(crate) fn diagnostic(message: &'static [u8]) {
    if let Some(mut file) = diagnostics() {
        let _ = file.write(message);
    }
}

pub(crate) fn output_ready(output: &File) -> io::Result<bool> {
    let mut descriptor = libc::pollfd {
        fd: output.as_raw_fd(),
        events: libc::POLLOUT,
        revents: 0,
    };
    // SAFETY: the owned output descriptor remains live; zero-time poll changes no flags.
    let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        return Err(io::Error::other("result consumer unavailable"));
    }
    Ok(descriptor.revents & libc::POLLOUT != 0)
}

pub(crate) fn poll(
    input: &File,
    output: &File,
    reading: bool,
    writing: bool,
) -> io::Result<(i16, i16)> {
    let mut descriptors = [
        libc::pollfd {
            fd: input.as_raw_fd(),
            events: if reading { libc::POLLIN } else { 0 },
            revents: 0,
        },
        libc::pollfd {
            fd: output.as_raw_fd(),
            events: if writing { libc::POLLOUT } else { 0 },
            revents: 0,
        },
    ];
    // SAFETY: the array contains two live, independently owned descriptors for the call's duration.
    let result = unsafe {
        libc::poll(
            descriptors.as_mut_ptr(),
            descriptors.len() as libc::nfds_t,
            25,
        )
    };
    if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
        return Err(io::Error::last_os_error());
    }
    Ok((descriptors[0].revents, descriptors[1].revents))
}
