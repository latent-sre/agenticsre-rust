use super::{CHILD_ENV, PreparedProcess};
use crate::RunControl;
use crate::result::{OperationResult, Status, timestamp};
use serde_json::json;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const GRACE: Duration = Duration::from_secs(2);
const FINAL_DRAIN: Duration = Duration::from_millis(500);
const POLL_MS: i32 = 20;

struct Capture<R> {
    pipe: Option<R>,
    bytes: Vec<u8>,
    observed: u64,
    truncated: bool,
    failed: bool,
    limit: usize,
}

impl<R: Read + AsRawFd> Capture<R> {
    fn new(pipe: R, limit: usize) -> io::Result<Self> {
        nonblocking(pipe.as_raw_fd())?;
        Ok(Self {
            pipe: Some(pipe),
            bytes: Vec::new(),
            observed: 0,
            truncated: false,
            failed: false,
            limit,
        })
    }

    fn fd(&self) -> RawFd {
        self.pipe.as_ref().map_or(-1, AsRawFd::as_raw_fd)
    }

    fn drain(&mut self) {
        let Some(pipe) = self.pipe.as_mut() else {
            return;
        };
        let mut buffer = [0_u8; 8192];
        // A finite turn prevents a continuous producer starving the other pipe or the clock.
        for _ in 0..8 {
            match pipe.read(&mut buffer) {
                Ok(0) => {
                    self.pipe = None;
                    break;
                }
                Ok(count) => {
                    self.observed = self.observed.saturating_add(count as u64);
                    let keep = count.min(self.limit.saturating_sub(self.bytes.len()));
                    self.bytes.extend_from_slice(&buffer[..keep]);
                    self.truncated |= keep < count;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => break,
                Err(_) => {
                    self.failed = true;
                    self.truncated = true;
                    self.pipe = None;
                    break;
                }
            }
        }
    }
}

fn nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fd belongs to a live child pipe; fcntl does not retain references.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: setting a live pipe's nonblocking flag does not change ownership.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn group_present(group: i32) -> io::Result<bool> {
    // SAFETY: group is the positive PID assigned to our child's own process group.
    if unsafe { libc::kill(-group, 0) } == 0 {
        return Ok(true);
    }
    match io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(io::Error::last_os_error()),
    }
}

fn signal_group(group: i32, signal: i32) -> io::Result<()> {
    // SAFETY: the child created a distinct process group with this positive PID. A
    // signal cannot select the runner's group. ESRCH means it already disappeared.
    if unsafe { libc::kill(-group, signal) } == 0
        || io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn consumer_closed(fd: Option<RawFd>) -> bool {
    let Some(fd) = fd else {
        return false;
    };
    let mut poll = libc::pollfd {
        fd,
        events: 0,
        revents: 0,
    };
    // SAFETY: poll points to one initialized stack element for the duration of the call.
    unsafe {
        libc::poll(&mut poll, 1, 0) > 0
            && poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
    }
}

fn wait_for_pipes(stdout: RawFd, stderr: RawFd) {
    let mut polls = [
        libc::pollfd {
            fd: stdout,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: stderr,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    // SAFETY: polls contains exactly two initialized elements; negative fds are ignored.
    unsafe {
        libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, POLL_MS);
    }
}

fn captured_text<R>(capture: &Option<Capture<R>>) -> (String, bool, bool) {
    if let Some(capture) = capture {
        let output = String::from_utf8_lossy(&capture.bytes);
        let replaced = matches!(output, std::borrow::Cow::Owned(_));
        (
            output.into_owned(),
            capture.truncated || capture.pipe.is_some(),
            replaced,
        )
    } else {
        (String::new(), true, false)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Stop {
    Timeout,
    Cancelled,
    ConsumerClosed,
    Descendants,
    Io,
}

impl Stop {
    fn name(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Cancelled => "cancellation",
            Self::ConsumerClosed => "result_consumer_closed",
            Self::Descendants => "descendants_after_leader_exit",
            Self::Io => "supervisor_io_error",
        }
    }
}

pub(crate) fn run(prepared: PreparedProcess, result: &mut OperationResult, control: &RunControl) {
    let begun = Instant::now();
    let deadline = begun + Duration::from_millis(result.effective_limits.timeout_ms);
    let mut command = Command::new(&prepared.executable);
    command
        .args(&prepared.input.args)
        .current_dir(&prepared.cwd)
        .env_clear()
        .envs(CHILD_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if let Some(launch) = &prepared.descriptor_launch
        && launch
            .configure(&mut command, &prepared.input.args)
            .is_err()
    {
        result.execution.status = Status::Failed;
        result.error(
            "descriptor_launch_failed",
            "The pinned executable handoff could not be prepared; no fallback was attempted.",
        );
        return;
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            result.execution.status = Status::Failed;
            result.error(
                "spawn_failed",
                "The native executable could not be started; it was not retried.",
            );
            return;
        }
    };
    result.effect_outcome = "unknown".into();
    result.coverage.scope =
        "Captured output and primary exit status of this invocation only".into();
    result.coverage.state = "complete".into();
    result.coverage.limitations.push("Process groups do not contain deliberately escaping programs; this is not an OS sandbox or a service health check.".into());
    let group = child.id() as i32;
    let limit = result.effective_limits.max_output_bytes;
    let stdout = Capture::new(child.stdout.take().expect("piped stdout"), limit);
    let stderr = Capture::new(child.stderr.take().expect("piped stderr"), limit);
    // If either fcntl fails, we still own the child and must terminate it without doing a
    // potentially blocking read. The common loop below handles this failure as a stop.
    let setup_failed = stdout.is_err() || stderr.is_err();
    let mut stdout = stdout.ok();
    let mut stderr = stderr.ok();
    let mut status: Option<ExitStatus> = None;
    let mut reason: Option<Stop> = None;
    let mut grace_deadline = None;
    let mut final_deadline = None;
    let mut term_sent = false;
    let mut kill_sent = false;
    let mut signal_failed = false;
    let mut wait_failed = false;
    let mut absent = false;
    let mut cancellation_signal = 0;
    loop {
        if let Some(stdout) = &mut stdout {
            stdout.drain();
        }
        if let Some(stderr) = &mut stderr {
            stderr.drain();
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(_) => wait_failed = true,
            }
        }
        if !absent {
            absent = matches!(group_present(group), Ok(false));
        }
        let pipes_closed = stdout.as_ref().is_none_or(|p| p.pipe.is_none())
            && stderr.as_ref().is_none_or(|p| p.pipe.is_none());
        let io_failed = setup_failed
            || wait_failed
            || stdout.as_ref().is_some_and(|p| p.failed)
            || stderr.as_ref().is_some_and(|p| p.failed);
        let now = Instant::now();
        let signal = control.signal.load(Ordering::Relaxed);
        // Signals received during an existing timeout/descendant cleanup still belong
        // to this invocation. Preserve the cleanup cause and primary child outcome.
        if signal != 0 {
            cancellation_signal = signal;
        }
        if reason.is_none() {
            reason = if signal != 0 {
                Some(Stop::Cancelled)
            } else if consumer_closed(control.result_fd) {
                Some(Stop::ConsumerClosed)
            } else if io_failed {
                Some(Stop::Io)
            } else if now >= deadline {
                Some(Stop::Timeout)
            } else if status.is_some() && absent && pipes_closed {
                break;
            } else if status.is_some() && !absent {
                Some(Stop::Descendants)
            } else {
                None
            };
            if reason.is_some() {
                if !absent {
                    term_sent = true;
                    signal_failed |= signal_group(group, libc::SIGTERM).is_err();
                }
                grace_deadline = Some(now + GRACE);
            }
        }
        if reason.is_some() {
            if status.is_some() && absent && pipes_closed {
                break;
            }
            if grace_deadline.is_some_and(|end| now >= end) && final_deadline.is_none() {
                if !absent {
                    kill_sent = true;
                    signal_failed |= signal_group(group, libc::SIGKILL).is_err();
                }
                final_deadline = Some(now + FINAL_DRAIN);
            }
            if final_deadline.is_some_and(|end| now >= end) {
                break;
            }
        }
        wait_for_pipes(
            stdout.as_ref().map_or(-1, Capture::fd),
            stderr.as_ref().map_or(-1, Capture::fd),
        );
    }
    // Only nonblocking final observations: escaped inherited pipes and an uninterruptible
    // child cannot turn cleanup into an unbounded wait or a detached reader thread.
    if status.is_none() {
        status = child.try_wait().ok().flatten();
    }
    if !absent {
        absent = matches!(group_present(group), Ok(false));
    }
    let stdout_closed = stdout.as_ref().is_some_and(|p| p.pipe.is_none());
    let stderr_closed = stderr.as_ref().is_some_and(|p| p.pipe.is_none());
    let (out, out_truncated, out_replaced) = captured_text(&stdout);
    let (err, err_truncated, err_replaced) = captured_text(&stderr);
    result.output.stdout = out;
    result.output.stderr = err;
    result.output.stdout_truncated = out_truncated;
    result.output.stderr_truncated = err_truncated;
    let replaced = out_replaced || err_replaced;
    if let Some(status) = status {
        result.execution.child_exit_code = status.code();
        result.execution.signal = status.signal().map(signal_name);
    }
    result.execution.status = match reason {
        Some(Stop::Timeout) => {
            result.error(
                "timed_out",
                "The whole-operation deadline expired, including output drainage.",
            );
            Status::TimedOut
        }
        Some(Stop::Cancelled) => {
            result.error(
                "cancelled",
                "The invocation was cancelled; owned process-group cleanup was attempted.",
            );
            Status::Cancelled
        }
        Some(Stop::ConsumerClosed) => {
            result.error(
                "result_consumer_closed",
                "The result consumer closed its pipe; owned process-group cleanup was attempted.",
            );
            Status::Cancelled
        }
        Some(Stop::Io) => {
            result.error(
                "supervisor_io_error",
                "Process supervision or output capture failed.",
            );
            Status::Unknown
        }
        _ if result.execution.child_exit_code != Some(0) => {
            result.error(
                "child_failed",
                "The primary process exited unsuccessfully or its exit could not be established.",
            );
            Status::Failed
        }
        Some(Stop::Descendants) => {
            result.error("descendants_terminated", "Descendants remained after the primary process exited; their group was terminated.");
            Status::Partial
        }
        _ => Status::Succeeded,
    };
    let cleanup_confirmed = status.is_some() && absent && stdout_closed && stderr_closed;
    if !cleanup_confirmed || signal_failed {
        result.error("cleanup_unconfirmed", "Cleanup could not be fully established within its bound; group presence can include zombies, and escaped processes are outside this supervisor's containment.");
        result.coverage.state = "partial".into();
        if result.execution.status == Status::Succeeded {
            result.execution.status = Status::Unknown;
        }
    }
    if replaced {
        result.output.encoding = "utf-8-replaced".into();
        result.error(
            "output_encoding_replaced",
            "Invalid or cut UTF-8 sequences were replaced in captured output.",
        );
        result.coverage.state = "partial".into();
        if result.execution.status == Status::Succeeded {
            result.execution.status = Status::Partial;
        }
    }
    if result.execution.status != Status::Succeeded {
        result.coverage.state = "partial".into();
    }
    result.data = json!({
        "process_id": group, "process_group_id": group, "effects": "unclassified", "profile": "operator-local",
        "cancellation_signal": if cancellation_signal == 0 { None } else { Some(cancellation_signal) },
        "stdout_bytes_observed": stdout.as_ref().map_or(0, |p| p.observed), "stderr_bytes_observed": stderr.as_ref().map_or(0, |p| p.observed),
        "cleanup": { "trigger": reason.map(Stop::name), "term_sent": term_sent, "kill_sent": kill_sent,
            "group_absent": absent, "primary_reaped": status.is_some(), "stdout_closed": stdout_closed, "stderr_closed": stderr_closed,
            "confirmed": cleanup_confirmed && !signal_failed, "grace_ms": 2000, "final_drain_ms": 500,
            "scope": "Observed process group and inherited output pipes only; deliberately escaped processes are not contained." }
    });
    result.sources.push(json!({"reference":"local:child-output", "observed_at": timestamp(), "evidence_state":"sourced", "untrusted":true}));
}

fn signal_name(signal: i32) -> String {
    match signal {
        libc::SIGINT => "SIGINT".into(),
        libc::SIGTERM => "SIGTERM".into(),
        libc::SIGKILL => "SIGKILL".into(),
        libc::SIGSEGV => "SIGSEGV".into(),
        libc::SIGABRT => "SIGABRT".into(),
        libc::SIGPIPE => "SIGPIPE".into(),
        other => format!("signal-{other}"),
    }
}
