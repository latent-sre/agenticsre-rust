//! Fixed Prometheus rule validation. Paths are snapshotted; only the reviewed binary runs.
use crate::{
    OperatorContext, RunControl,
    request::{Problem, Request},
    result::{OperationResult, Status},
};
use serde_json::{Value, json};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
const VERSION: &str = "3.15.0";
const REVISION: &str = "5241a27fe3c6983549fccc32f6e65917408c63cd";
const BINARY_SHA: &str = "sha256:c736d55d3ccd959fe48329965fb5cb671d45585a9483954766448035100ff75c";
const BINARY_BYTES: u64 = 119_146_763;
const MAX_DIAGNOSTICS: usize = 256 * 1024;
const RULES: [&str; 5] = [
    "strict_yaml_known_fields",
    "rule_schema",
    "promql_syntax",
    "template_syntax",
    "duplicate_rules",
];
const UNSUPPORTED: [&str; 5] = [
    "live_rule_evaluation",
    "metric_existence",
    "alert_routing",
    "template_execution",
    "future_rule_formats",
];
const SCOPE: &str = "Offline syntax/schema and duplicate lint for the exact consumed Prometheus rule bytes; no live evaluation or deployment-health claim.";
const DESCRIPTOR: &str = concat!(
    "save-config-promtool-v1\n",
    "runtime=promtool 3.15.0 linux-amd64\n",
    "binary=sha256:c736d55d3ccd959fe48329965fb5cb671d45585a9483954766448035100ff75c\n",
    "argv0=promtool\nargs=check rules --lint=duplicate-rules --lint-fatal /proc/self/fd/{sealed_model_fd}\n",
    "input_max_bytes=2097152\nexecutable_max_bytes=134217728\n",
    "executable_snapshot=private-unlinked-readonly-file\nmodel_snapshot=sealed-memfd\nstdin=null\nenvironment=workbench-minimal-v1\n",
    "success=exact-count-output,no-stderr,positive-rule-count\n",
    "lint=exit3\nmultiple-document-warning=incomplete\n"
);
fn binding_digest() -> String {
    // Bind both the declared native protocol and compiled adapter implementation.
    super::digest(&[DESCRIPTOR.as_bytes(), include_bytes!("prometheus_rules.rs")].concat())
}
fn envelope(request: &Request) -> Value {
    json!({"kind":"prometheus-rules","validator":{"id":"promtool","version":1,"checker_digest":BINARY_SHA,"binding_digest":binding_digest(),"upstream_revision":REVISION},
        "input":{"file":request.inputs["file"],"digest":null,"bytes":null},"implemented_rules":RULES,"unsupported_sections":UNSUPPORTED,
        "validation_state":"not_established","validation_outcome":"unknown","checked_rules":null,"findings":[],"findings_total":0,
        "diagnostics":{"stdout":"","stderr":"","untrusted":true,"truncated":false},"runtime":null,"cancellation_signal":null})
}
fn active(control: &RunControl, deadline: Instant) -> Result<(), &'static str> {
    if control.signal.load(Ordering::Relaxed) != 0 {
        return Err("cancelled");
    }
    if Instant::now() >= deadline {
        return Err("deadline_exceeded");
    }
    #[cfg(target_os = "linux")]
    if let Some(fd) = control.result_fd {
        let mut poll = libc::pollfd {
            fd,
            events: 0,
            revents: 0,
        };
        // SAFETY: one initialized descriptor is borrowed for nonblocking poll.
        if unsafe { libc::poll(&mut poll, 1, 0) } > 0
            && poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
        {
            return Err("result_consumer_closed");
        }
    }
    Ok(())
}
fn stopped(code: &'static str, control: &RunControl, result: &mut OperationResult) {
    result.execution.status = if code == "deadline_exceeded" {
        Status::TimedOut
    } else {
        Status::Cancelled
    };
    result.coverage.state = "partial".into();
    result.data["validation_state"] = json!("not_established");
    result.data["validation_outcome"] = json!("unknown");
    let signal = control.signal.load(Ordering::Relaxed);
    result.data["cancellation_signal"] = if signal == 0 {
        Value::Null
    } else {
        json!(signal)
    };
    result.error(code,"Prometheus rule validation stopped within its invocation budget; no complete validation is claimed.");
}
fn reject(
    result: &mut OperationResult,
    code: &'static str,
    message: &'static str,
    unsupported: bool,
) {
    if unsupported {
        result.data["validation_state"] = json!("unsupported");
        result.reject(Problem::unsupported(code, message));
    } else {
        result.reject(Problem::invalid(code, message));
    }
}
pub(super) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    result.data = envelope(request);
    result.coverage.scope = SCOPE.into();
    result.coverage.limitations=vec!["The pinned native validator runs under the invoking account with the existing process supervisor; this is not a new filesystem or network sandbox.".into(),"Native diagnostics are untrusted model-derived text, not structured findings or current-state facts. Findings counts summarize adapter outcomes, not individual native errors.".into(),"Live evaluation, metric existence, routing, template execution and future rule formats are not validated.".into()];
    if request.inputs["validator"]
        .get("expected_binding_sha256")
        .is_some_and(|pin| pin != &result.data["validator"]["binding_digest"])
    {
        reject(
            result,
            "validator_digest_mismatch",
            "The compiled rule-validator binding does not match its expected digest; no executable or model was read.",
            false,
        );
        return;
    }
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    if let Err(code) = active(control, deadline) {
        stopped(code, control, result);
        return;
    }
    #[cfg(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_pointer_width = "64"
    ))]
    native::run(request, control, context, deadline, result);
    #[cfg(not(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_pointer_width = "64"
    )))]
    {
        let _ = context;
        reject(
            result,
            "unsupported_platform",
            "The reviewed promtool binding requires Linux amd64 and sealed descriptor execution.",
            true,
        );
    }
}
fn finding(result: &mut OperationResult, rule: &str, detail: &str) {
    result.data["findings"] = json!([{"rule":rule,"location":"input","detail":detail}]);
    result.data["findings_total"] = json!(1);
}
fn exact_count(stdout: &str, model_argument: &str) -> Option<u64> {
    let prefix = format!("Checking {model_argument}\n  SUCCESS: ");
    let value = stdout
        .strip_prefix(&prefix)?
        .strip_suffix(" rules found\n\n")?;
    if value.is_empty()
        || value.len() > 10
        || !value.bytes().all(|b| b.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return None;
    }
    value.parse().ok()
}
fn interpret(result: &mut OperationResult, model_argument: &str) {
    let stdout = result.data["diagnostics"]["stdout"].as_str().unwrap_or("");
    let stderr = result.data["diagnostics"]["stderr"].as_str().unwrap_or("");
    // Only expected ordinary exits can be reclassified. Cleanup, signal, encoding
    // and other supervisor uncertainty never become an adapter success.
    if matches!(
        result.execution.status,
        Status::Cancelled | Status::TimedOut | Status::Unknown
    ) {
        return;
    }
    let lost = result.data["diagnostics"]["truncated"] == true || result.output.encoding != "utf-8";
    let uncertain = result
        .errors
        .iter()
        .any(|error| error.code != "child_failed")
        || result.data["supervisor"]["cleanup"]["confirmed"] != true;
    if lost || uncertain {
        result.execution.status = Status::Partial;
        result.coverage.state = "partial".into();
        result.data["validation_state"] = json!("partial");
        result.error("promtool_observation_incomplete","Native output or supervision is incomplete; no complete validation result was accepted.");
        return;
    }
    if stderr.contains("Multiple document yaml rules files are not supported") {
        result.execution.status = Status::Partial;
        result.coverage.state = "partial".into();
        result.data["validation_state"] = json!("partial");
        result.error("promtool_partial_input","The native validator warned that it did not process the whole model; diagnostics are retained and validation remains unknown.");
        return;
    }
    match result.execution.child_exit_code {
        Some(0) if !stderr.is_empty() => {
            result.execution.status = Status::Partial;
            result.coverage.state = "partial".into();
            result.data["validation_state"] = json!("partial");
            result.error("promtool_warning","The native validator emitted warnings despite exit zero; no complete validation result was accepted.");
        }
        Some(0) => {
            match exact_count(stdout, model_argument) {
                Some(count) if count > 0 => {
                    result.execution.status = Status::Succeeded;
                    result.coverage.state = "complete".into();
                    result.data["validation_state"] = json!("complete");
                    result.data["validation_outcome"] = json!("valid");
                    result.data["checked_rules"] = json!(count);
                }
                Some(0) => {
                    result.execution.status = Status::Failed;
                    result.coverage.state = "not_established".into();
                    result.data["validation_outcome"] = json!("uncheckable");
                    result.data["checked_rules"] = json!(0);
                    finding(
                        result,
                        "no-rules",
                        "The input contains no rules; empty validation is not a pass.",
                    );
                    result.error(
                        "uncheckable_rules",
                        "The native validator found no rules to check.",
                    );
                }
                _ => {
                    result.execution.status = Status::Failed;
                    result.coverage.state = "not_established".into();
                    result.error("invalid_promtool_response","The pinned validator did not produce the exact supported success protocol.");
                }
            }
        }
        Some(1)
            if stdout == format!("Checking {model_argument}\n")
                && stderr.starts_with("  FAILED:\n") =>
        {
            result.execution.status = Status::Failed;
            result.coverage.state = "not_established".into();
            result.data["validation_outcome"] = json!("invalid");
            finding(
                result,
                "promtool-invalid",
                "The native validator rejected the rules; consult bounded untrusted diagnostics for details.",
            );
            result.errors.retain(|error| error.code != "child_failed");
            result.error(
                "invalid_prometheus_rules",
                "The native validator rejected the supplied rule model.",
            );
        }
        Some(3)
            if stdout == format!("Checking {model_argument}\n\n")
                && stderr.starts_with("  FAILED:\nlint error ")
                && stderr.contains(" duplicate rule(s) found.\n") =>
        {
            result.execution.status = Status::Succeeded;
            result.coverage.state = "complete".into();
            result.data["validation_state"] = json!("complete");
            result.data["validation_outcome"] = json!("lint_findings");
            result.errors.retain(|error| error.code != "child_failed");
            finding(
                result,
                "duplicate-rules",
                "The configured duplicate-rule lint reported findings; native diagnostic counts are not interpreted as structured findings.",
            );
        }
        Some(1 | 3) => {
            result.execution.status = Status::Failed;
            result.coverage.state = "not_established".into();
            result.error(
                "invalid_promtool_response",
                "The pinned validator did not produce the supported failure or lint protocol.",
            );
        }
        _ => {
            result.execution.status = Status::Failed;
            result.coverage.state = "not_established".into();
            result.error(
                "unexpected_promtool_exit",
                "The native validator did not return a supported ordinary validation exit.",
            );
        }
    }
}

#[cfg(all(
    target_os = "linux",
    target_arch = "x86_64",
    target_pointer_width = "64"
))]
mod native {
    use super::*;
    use crate::{
        process::{self, DescriptorLaunch, PreparedProcess},
        request::ProcessInput,
    };
    use sha2::{Digest, Sha256};
    use std::{
        fs::{File, OpenOptions, Permissions},
        io::{Read, Seek, SeekFrom, Write},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        },
        path::{Path, PathBuf},
    };
    const MAX_BINARY: usize = 128 * 1024 * 1024;
    const MAX_MODEL: usize = 2 * 1024 * 1024;
    struct Snapshot {
        file: File,
        digest: String,
        bytes: u64,
    }
    // The execution profile permits ordinary file-descriptor execution, but may deny
    // executable memfds. Remove our private names before the bounded copy starts;
    // only this writer and a separate read-only description retain the new inode.
    fn executable_file() -> Result<(File, File), &'static str> {
        use crate::evidence::store::fs::Directory;
        const UNAVAILABLE: &str = "executable_snapshot_unavailable";
        const CLEANUP: &str = "executable_snapshot_cleanup_failed";
        let parent = Directory(
            OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open("/tmp")
                .map_err(|_| UNAVAILABLE)?,
        );
        let name = crate::result::new_id("promtool-snapshot");
        let directory = match parent.create_dir(&name) {
            Ok(directory) => directory,
            Err(error) => {
                if error.effects && parent.unlink(&name, true).is_err() {
                    return Err(CLEANUP);
                }
                return Err(UNAVAILABLE);
            }
        };
        let mut named_file = false;
        let mut named_directory = true;
        let prepared = (|| {
            let writer = directory.create_file("executable").map_err(|error| {
                named_file = error.effects;
                UNAVAILABLE
            })?;
            named_file = true;
            let reader = directory
                .open_file("executable", false)
                .map_err(|_| UNAVAILABLE)?;
            let written = writer.metadata().map_err(|_| UNAVAILABLE)?;
            let read = reader.metadata().map_err(|_| UNAVAILABLE)?;
            if written.dev() != read.dev() || written.ino() != read.ino() {
                return Err(UNAVAILABLE);
            }
            directory
                .unlink("executable", false)
                .map_err(|_| UNAVAILABLE)?;
            named_file = false;
            parent.unlink(&name, true).map_err(|_| UNAVAILABLE)?;
            named_directory = false;
            Ok((writer, reader))
        })();
        let file_cleanup = !named_file || directory.unlink("executable", false).is_ok();
        let directory_cleanup = !named_directory || parent.unlink(&name, true).is_ok();
        if !file_cleanup || !directory_cleanup {
            return Err(CLEANUP);
        }
        prepared
    }
    fn snapshot(
        path: &Path,
        max: usize,
        executable: bool,
        control: &RunControl,
        deadline: Instant,
    ) -> Result<Snapshot, &'static str> {
        active(control, deadline)?;
        let unreadable = if executable {
            "promtool_unavailable"
        } else {
            "model_unreadable"
        };
        let mut input = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(path)
            .map_err(|_| unreadable)?;
        let metadata = input.metadata().map_err(|_| unreadable)?;
        if !metadata.is_file() {
            return Err(if executable {
                "unsupported_promtool_binary"
            } else {
                "non_regular_model"
            });
        }
        if metadata.len() > max as u64 {
            return Err(if executable {
                "unsupported_promtool_binary"
            } else {
                "model_too_large"
            });
        }
        let snapshot_error = if executable {
            "executable_snapshot_unavailable"
        } else {
            "sealed_snapshot_unavailable"
        };
        let (mut file, reader) = if executable {
            let (writer, reader) = executable_file()?;
            (writer, Some(reader))
        } else {
            let flags = libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING | libc::MFD_NOEXEC_SEAL;
            // SAFETY: fixed NUL-terminated name; successful syscall returns an owned fd.
            let descriptor = unsafe { libc::memfd_create(c"config-snapshot".as_ptr(), flags) };
            if descriptor < 0 {
                return Err(snapshot_error);
            }
            // SAFETY: successful memfd_create transfers unique ownership.
            (unsafe { File::from_raw_fd(descriptor) }, None)
        };
        let mut hasher = Sha256::new();
        let mut bytes = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            active(control, deadline)?;
            let allowed = ((max as u64 + 1 - bytes).min(buffer.len() as u64)) as usize;
            let size = input.read(&mut buffer[..allowed]).map_err(|_| unreadable)?;
            if size == 0 {
                break;
            }
            bytes += size as u64;
            if bytes > max as u64 {
                return Err(if executable {
                    "unsupported_promtool_binary"
                } else {
                    "model_too_large"
                });
            }
            hasher.update(&buffer[..size]);
            file.write_all(&buffer[..size])
                .map_err(|_| snapshot_error)?;
        }
        let file = if let Some(reader) = reader {
            file.set_permissions(Permissions::from_mode(0o500))
                .map_err(|_| snapshot_error)?;
            // Close the only writable description before descriptor execution, avoiding
            // ETXTBSY. The reader's offset is still zero and its names are already gone.
            drop(file);
            reader
        } else {
            file.seek(SeekFrom::Start(0)).map_err(|_| snapshot_error)?;
            // SAFETY: fcntl seals our live memfd against all writes and size changes.
            if unsafe {
                libc::fcntl(
                    file.as_raw_fd(),
                    libc::F_ADD_SEALS,
                    libc::F_SEAL_WRITE
                        | libc::F_SEAL_GROW
                        | libc::F_SEAL_SHRINK
                        | libc::F_SEAL_SEAL,
                )
            } < 0
            {
                return Err(snapshot_error);
            }
            file
        };
        // SAFETY: duplicate our owned snapshot above stdio; the original then closes.
        let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        if fd < 0 {
            return Err(snapshot_error);
        }
        // SAFETY: successful duplication returned unique descriptor ownership.
        let file = unsafe { File::from_raw_fd(fd) };
        active(control, deadline)?;
        use std::fmt::Write as _;
        let mut digest = String::from("sha256:");
        for byte in hasher.finalize() {
            write!(&mut digest, "{byte:02x}").expect("string writing");
        }
        Ok(Snapshot {
            file,
            digest,
            bytes,
        })
    }
    fn problem(
        code: &'static str,
        control: &RunControl,
        result: &mut OperationResult,
        executable: bool,
    ) {
        if matches!(
            code,
            "cancelled" | "deadline_exceeded" | "result_consumer_closed"
        ) {
            stopped(code, control, result);
        } else if code == "executable_snapshot_cleanup_failed" {
            result.execution.status = Status::Failed;
            result.effect_outcome = "unknown".into();
            result.coverage.limitations.push("A private temporary executable snapshot could not be fully removed; local residue may remain. No model was read or child started.".into());
            result.error(
                code,
                "Temporary executable snapshot cleanup failed; local effects are unknown.",
            );
        } else if executable {
            reject(
                result,
                code,
                "The explicitly configured validator could not be admitted as the exact reviewed binary; no model was read or child started.",
                code != "unsupported_promtool_binary",
            );
        } else {
            result.execution.status = if code == "sealed_snapshot_unavailable" {
                Status::Unsupported
            } else {
                Status::Failed
            };
            result.error(code,"The bounded model snapshot could not be prepared; no native validation was performed.");
        }
    }
    pub(super) fn run(
        request: &Request,
        control: &RunControl,
        context: &OperatorContext,
        deadline: Instant,
        result: &mut OperationResult,
    ) {
        let Some(path) = &context.promtool_path else {
            reject(
                result,
                "promtool_required",
                "Attach an explicit --promtool path to the reviewed native validator; no model was read.",
                true,
            );
            return;
        };
        if !path.is_absolute()
            || path.to_str().is_none_or(|s| {
                s.is_empty() || s.len() > 4096 || s.bytes().any(|b| b.is_ascii_control())
            })
        {
            reject(
                result,
                "invalid_promtool_path",
                "The trusted promtool startup path must be an absolute bounded path without controls.",
                false,
            );
            return;
        }
        let executable = match snapshot(path, MAX_BINARY, true, control, deadline) {
            Ok(file) => file,
            Err(code) => {
                problem(code, control, result, true);
                return;
            }
        };
        if executable.digest != BINARY_SHA || executable.bytes != BINARY_BYTES {
            reject(
                result,
                "unsupported_promtool_binary",
                "The configured executable does not match reviewed promtool 3.15.0 linux-amd64; no model was read or child started.",
                false,
            );
            return;
        }
        result.data["runtime"] = json!({"executable":path,"version":VERSION,"revision":REVISION,"executable_sha256":BINARY_SHA});
        let model = match snapshot(
            Path::new(request.inputs["file"].as_str().unwrap()),
            MAX_MODEL,
            false,
            control,
            deadline,
        ) {
            Ok(file) => file,
            Err(code) => {
                problem(code, control, result, false);
                return;
            }
        };
        result.data["input"]["digest"] = json!(model.digest);
        result.data["input"]["bytes"] = json!(model.bytes);
        result.sources = vec![
            json!({"reference":format!("file:{}@{}",request.inputs["file"].as_str().unwrap(),model.digest),"observed_at":result.started_at,"evidence_state":"sourced","untrusted":true}),
        ];
        if request
            .inputs
            .get("expected_input_sha256")
            .is_some_and(|pin| pin != &model.digest)
        {
            result.execution.status = Status::Failed;
            result.error("input_digest_mismatch","The consumed rule bytes do not match the expected input digest; no native validator was started.");
            return;
        }
        if let Err(code) = active(control, deadline) {
            stopped(code, control, result);
            return;
        }
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(request.limits.timeout_ms as u128) as u64;
        if remaining == 0 {
            stopped("deadline_exceeded", control, result);
            return;
        }
        let model_argument = format!("/proc/self/fd/{}", model.file.as_raw_fd());
        let args = vec![
            "check".into(),
            "rules".into(),
            "--lint=duplicate-rules".into(),
            "--lint-fatal".into(),
            model_argument.clone(),
        ];
        let prepared = PreparedProcess {
            input: ProcessInput {
                program: path.to_string_lossy().into_owned(),
                args,
                cwd: "/".into(),
            },
            executable: path.clone(),
            cwd: PathBuf::from("/"),
            descriptor_launch: Some(DescriptorLaunch {
                argv0: "promtool",
                executable: executable.file,
                inherited: vec![model.file],
            }),
        };
        result.resolved_target = json!({"executable":path,"cwd":"/","effects":"observe","resolution":"private unlinked readonly executable snapshot verified by binary SHA-256","validator":"promtool","executable_sha256":BINARY_SHA});
        result.effective_limits.timeout_ms = remaining;
        // The supervisor uses this per stream, reserving an aggregate diagnostics bound.
        let budget = request.limits.max_output_bytes.min(MAX_DIAGNOSTICS);
        result.effective_limits.max_output_bytes = budget / 2;
        let data = std::mem::replace(&mut result.data, json!({}));
        process::run(prepared, result, control);
        result.effective_limits.max_output_bytes = budget;
        let mut supervisor = std::mem::replace(&mut result.data, data);
        if let Some(map) = supervisor.as_object_mut() {
            map.remove("effects");
            map.remove("profile");
        }
        if supervisor.get("process_id").is_some() {
            result.data["cancellation_signal"] = supervisor
                .get("cancellation_signal")
                .cloned()
                .unwrap_or(Value::Null);
            result.data["supervisor"] = supervisor;
        }
        let stdout = std::mem::take(&mut result.output.stdout);
        let stderr = std::mem::take(&mut result.output.stderr);
        result.data["diagnostics"] = json!({"stdout":stdout,"stderr":stderr,"untrusted":true,"truncated":result.output.stdout_truncated||result.output.stderr_truncated||result.output.encoding!="utf-8"});
        result.coverage.scope = SCOPE.into();
        if result.effect_outcome != "not_attempted" {
            result.effect_outcome = "not_applicable".into();
        }
        if result.data.get("supervisor").is_some() {
            interpret(result, &model_argument);
        }
        if let Err(code) = active(control, deadline)
            && !matches!(
                result.execution.status,
                Status::Cancelled | Status::TimedOut
            )
        {
            stopped(code, control, result);
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn executable_snapshot_is_unlinked_readonly_and_independent_of_source() {
            let path = std::env::temp_dir().join(crate::result::new_id("executable-source"));
            std::fs::write(&path, b"original executable bytes").unwrap();
            let mut observed = snapshot(
                &path,
                MAX_BINARY,
                true,
                &RunControl::default(),
                Instant::now() + Duration::from_secs(10),
            )
            .unwrap();
            std::fs::remove_file(&path).unwrap();
            std::fs::write(&path, b"replacement bytes").unwrap();
            let mut bytes = Vec::new();
            observed.file.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"original executable bytes");
            assert_eq!(observed.digest, super::super::super::digest(&bytes));
            let metadata = observed.file.metadata().unwrap();
            assert_eq!(metadata.nlink(), 0);
            assert_eq!(metadata.permissions().mode() & 0o777, 0o500);
            assert_eq!(
                observed.file.write_all(b"x").unwrap_err().raw_os_error(),
                Some(libc::EBADF)
            );
            // SAFETY: obtain access flags from our live read-only descriptor.
            let flags = unsafe { libc::fcntl(observed.file.as_raw_fd(), libc::F_GETFL) };
            assert_eq!(flags & libc::O_ACCMODE, libc::O_RDONLY);
            let former =
                std::fs::read_link(format!("/proc/self/fd/{}", observed.file.as_raw_fd())).unwrap();
            assert!(former.to_string_lossy().ends_with("/executable (deleted)"));
            assert!(!former.parent().unwrap().exists());
            std::fs::remove_file(path).unwrap();
        }

        #[test]
        fn unlinked_executable_snapshot_runs_through_supervisor_without_source_path() {
            // Some hosts install a uutils shim that rejects descriptor execution.
            // Prefer the ordinary GNU fixture there; standard Linux uses /usr/bin/printf.
            let fixture = if Path::new("/usr/bin/gnuprintf").exists() {
                "/usr/bin/gnuprintf"
            } else {
                "/usr/bin/printf"
            };
            let source = std::fs::canonicalize(fixture).unwrap();
            let control = RunControl::default();
            let snapshot = snapshot(
                &source,
                MAX_BINARY,
                true,
                &control,
                Instant::now() + Duration::from_secs(10),
            )
            .unwrap();
            let request = Request::new("config.check", json!({}));
            let mut result = OperationResult::new(&request);
            let prepared = PreparedProcess {
                input: ProcessInput {
                    program: "/definitely/absent/executable".into(),
                    args: vec!["%s".into(), "snapshot-executed".into()],
                    cwd: "/".into(),
                },
                executable: PathBuf::from("/definitely/absent/executable"),
                cwd: PathBuf::from("/"),
                descriptor_launch: Some(DescriptorLaunch {
                    argv0: "printf",
                    executable: snapshot.file,
                    inherited: vec![],
                }),
            };
            process::run(prepared, &mut result, &control);
            assert_eq!(
                result.execution.status,
                Status::Succeeded,
                "errors: {:?}; stderr: {}",
                result.errors,
                result.output.stderr
            );
            assert_eq!(
                result.execution.child_exit_code,
                Some(0),
                "stderr: {}",
                result.output.stderr
            );
            assert_eq!(result.output.stdout, "snapshot-executed");
            assert!(result.output.stderr.is_empty());
        }

        #[test]
        fn snapshot_refuses_symlinks_oversize_and_expired_budget() {
            use std::os::unix::fs::symlink;
            let path = std::env::temp_dir().join(crate::result::new_id("snapshot-refusal"));
            std::fs::write(&path, b"0123456789").unwrap();
            let link = path.with_extension("link");
            symlink(&path, &link).unwrap();
            let control = RunControl::default();
            let deadline = Instant::now() + Duration::from_secs(10);
            assert_eq!(
                snapshot(&link, MAX_MODEL, false, &control, deadline).err(),
                Some("model_unreadable")
            );
            assert_eq!(
                snapshot(&path, 5, false, &control, deadline).err(),
                Some("model_too_large")
            );
            assert_eq!(
                snapshot(&path, MAX_MODEL, false, &control, Instant::now()).err(),
                Some("deadline_exceeded")
            );
            control.signal.store(2, Ordering::Relaxed);
            assert_eq!(
                snapshot(&path, MAX_MODEL, false, &control, deadline).err(),
                Some("cancelled")
            );
            std::fs::remove_file(link).unwrap();
            std::fs::remove_file(path).unwrap();
        }

        #[test]
        fn sealed_snapshot_preserves_original_bytes_and_denies_writes_resize_or_reseal() {
            let path = std::env::temp_dir().join(crate::result::new_id("snapshot"));
            std::fs::write(&path, b"groups: []\n").unwrap();
            let mut observed = snapshot(
                &path,
                MAX_MODEL,
                false,
                &RunControl::default(),
                Instant::now() + Duration::from_secs(10),
            )
            .unwrap();
            std::fs::write(&path, b"different bytes").unwrap();
            let mut bytes = Vec::new();
            observed.file.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"groups: []\n");
            assert_eq!(observed.digest, super::super::super::digest(&bytes));
            assert_eq!(
                observed.file.write_all(b"x").unwrap_err().raw_os_error(),
                Some(libc::EPERM)
            );
            assert_eq!(
                observed.file.set_len(0).unwrap_err().raw_os_error(),
                Some(libc::EPERM)
            );
            // SAFETY: reads the seal mask of this owned live memfd.
            let seals = unsafe { libc::fcntl(observed.file.as_raw_fd(), libc::F_GET_SEALS) };
            assert_eq!(
                seals
                    & (libc::F_SEAL_WRITE
                        | libc::F_SEAL_GROW
                        | libc::F_SEAL_SHRINK
                        | libc::F_SEAL_SEAL),
                libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL
            );
            std::fs::remove_file(path).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn receipt(exit: i32, out: &str, err: &str) -> OperationResult {
        let request = Request::new(
            "config.check",
            json!({"kind":"prometheus-rules","validator":{"id":"promtool","version":1},"file":"/tmp/rules.yml"}),
        );
        let mut result = OperationResult::new(&request);
        result.data = envelope(&request);
        result.data["diagnostics"]["stdout"] = json!(out);
        result.data["diagnostics"]["stderr"] = json!(err);
        result.data["supervisor"] = json!({"cleanup":{"confirmed":true}});
        result.execution.child_exit_code = Some(exit);
        result.execution.status = if exit == 0 {
            Status::Succeeded
        } else {
            Status::Failed
        };
        if exit != 0 {
            result.error("child_failed", "child failed");
        }
        result
    }
    #[test]
    fn exact_native_success_requires_positive_count_and_no_stderr() {
        let arg = "/proc/self/fd/10";
        let mut result = receipt(
            0,
            "Checking /proc/self/fd/10\n  SUCCESS: 2 rules found\n\n",
            "",
        );
        interpret(&mut result, arg);
        assert_eq!(result.execution.status, Status::Succeeded);
        assert_eq!(result.data["checked_rules"], 2);
        assert_eq!(result.data["validation_outcome"], "valid");
        for (out, err, state) in [
            (
                "Checking /proc/self/fd/10\n  SUCCESS: 0 rules found\n\n",
                "",
                "uncheckable",
            ),
            (
                "Checking /wrong\n  SUCCESS: 2 rules found\n\n",
                "",
                "unknown",
            ),
            (
                "Checking /proc/self/fd/10\n  SUCCESS: 2 rules found\n\n",
                "warning",
                "unknown",
            ),
        ] {
            let mut r = receipt(0, out, err);
            interpret(&mut r, arg);
            assert_ne!(r.execution.status, Status::Succeeded);
            assert_eq!(r.data["validation_outcome"], state);
        }
    }
    #[test]
    fn native_failures_lint_and_incomplete_observations_preserve_child_outcomes() {
        let mut invalid = receipt(1, "Checking /proc/self/fd/10\n", "  FAILED:\nuntrusted");
        interpret(&mut invalid, "/proc/self/fd/10");
        assert_eq!(invalid.execution.status, Status::Failed);
        assert_eq!(invalid.data["validation_outcome"], "invalid");
        assert_eq!(invalid.data["findings_total"], 1);
        let mut lint = receipt(
            3,
            "Checking /proc/self/fd/10\n\n",
            "  FAILED:\nlint error 1 duplicate rule(s) found.\nMetric: example\n",
        );
        interpret(&mut lint, "/proc/self/fd/10");
        assert_eq!(lint.execution.status, Status::Succeeded);
        assert_eq!(lint.execution.child_exit_code, Some(3));
        assert_eq!(lint.data["validation_outcome"], "lint_findings");
        assert!(lint.errors.is_empty());
        for exit in [0, 3] {
            let mut r = receipt(
                exit,
                "Checking /proc/self/fd/10\n\n",
                "Multiple document yaml rules files are not supported",
            );
            interpret(&mut r, "/proc/self/fd/10");
            assert_eq!(r.execution.status, Status::Partial);
            assert_eq!(r.data["validation_outcome"], "unknown");
        }
        for exit in [1, 3, 4] {
            let mut malformed = receipt(exit, "unexpected output", "unexpected diagnostics");
            interpret(&mut malformed, "/proc/self/fd/10");
            assert_eq!(malformed.execution.status, Status::Failed);
            assert_eq!(malformed.data["validation_outcome"], "unknown");
            assert_eq!(malformed.data["findings_total"], 0);
        }
        let mut timed = receipt(0, "", " ");
        timed.execution.status = Status::TimedOut;
        interpret(&mut timed, "/proc/self/fd/10");
        assert_eq!(timed.execution.status, Status::TimedOut);
        let mut lost = receipt(
            0,
            "Checking /proc/self/fd/10\n  SUCCESS: 2 rules found\n\n",
            "",
        );
        lost.data["diagnostics"]["truncated"] = json!(true);
        interpret(&mut lost, "/proc/self/fd/10");
        assert_eq!(lost.execution.status, Status::Partial);
        assert!(lost.data["checked_rules"].is_null());
    }
}
