use super::*;
use crate::{
    evidence::store::fs::Directory,
    process::{self, DescriptorLaunch, PreparedProcess},
    request::ProcessInput,
};
use std::{
    fs::{File, OpenOptions, Permissions},
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::PathBuf,
};
fn snapshot(bytes: &[u8], control: &RunControl, deadline: Instant) -> Result<File, Fault> {
    active(control, deadline)?;
    let unavailable = || Fault::new("extension_snapshot_unavailable");
    let parent = Directory(
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open("/tmp")
            .map_err(|_| unavailable())?,
    );
    let name = crate::result::new_id("extension-snapshot");
    let directory = match parent.create_dir(&name) {
        Ok(directory) => directory,
        Err(error) => {
            if error.effects && parent.unlink(&name, true).is_err() {
                return Err(Fault::new("extension_snapshot_cleanup_failed").after_effects());
            }
            return Err(unavailable());
        }
    };
    let mut named_file = false;
    let mut named_directory = true;
    let prepared = (|| {
        let writer = directory.create_file("executable").map_err(|e| {
            named_file = e.effects;
            unavailable()
        })?;
        named_file = true;
        let reader = directory
            .open_file("executable", false)
            .map_err(|_| unavailable())?;
        let written = writer.metadata().map_err(|_| unavailable())?;
        let read = reader.metadata().map_err(|_| unavailable())?;
        if written.dev() != read.dev() || written.ino() != read.ino() {
            return Err(unavailable());
        }
        directory
            .unlink("executable", false)
            .map_err(|_| unavailable())?;
        named_file = false;
        parent.unlink(&name, true).map_err(|_| unavailable())?;
        named_directory = false;
        Ok((writer, reader))
    })();
    let file_clean = !named_file || directory.unlink("executable", false).is_ok();
    let directory_clean = !named_directory || parent.unlink(&name, true).is_ok();
    if !file_clean || !directory_clean {
        return Err(Fault::new("extension_snapshot_cleanup_failed").after_effects());
    }
    let (mut writer, reader) = prepared?;
    for chunk in bytes.chunks(64 * 1024) {
        active(control, deadline)?;
        writer.write_all(chunk).map_err(|_| unavailable())?;
    }
    writer
        .set_permissions(Permissions::from_mode(0o500))
        .map_err(|_| unavailable())?;
    drop(writer);
    // SAFETY: duplicate a live owned readonly descriptor above stdio; no path is reopened.
    let fd = unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    if fd < 0 {
        return Err(unavailable());
    }
    // SAFETY: successful duplication transfers unique ownership.
    let reader = unsafe { File::from_raw_fd(fd) };
    active(control, deadline)?;
    Ok(reader)
}
fn envelope() -> Value {
    json!({"format_version":1,"package":null,"capability":null,"authorization":null,
        "protocol":{"name":"save-extension","version":1,"state":"not_started","frames_received":0,"terminal_received":false,"error":null},
        "progress":[],"terminal":null,"diagnostics":{"stderr":"","untrusted":true,"truncated":false},
        "omitted_progress":0,"terminal_data_omitted":false,"terminal_error_omitted":false,
        "detail_bytes":0,"detail_budget_bytes":0,"supervisor":null,"cancellation_signal":null})
}
fn detail_bytes(data: &Value) -> usize {
    serde_json::to_vec(&json!({"progress":data["progress"],"terminal_data":data["terminal"]["data"],"terminal_error_message":data["terminal"]["error"]["message"],"stderr":data["diagnostics"]["stderr"]})).expect("detail").len()
}
fn bound(result: &mut OperationResult) {
    let budget = result.effective_limits.max_output_bytes;
    let total = result.data["progress"].as_array().unwrap().len();
    while detail_bytes(&result.data) > budget
        && !result.data["progress"].as_array().unwrap().is_empty()
    {
        result.data["progress"].as_array_mut().unwrap().pop();
    }
    result.data["omitted_progress"] =
        json!(total - result.data["progress"].as_array().unwrap().len());
    if detail_bytes(&result.data) > budget && result.data["terminal"]["data"].is_object() {
        result.data["terminal"]["data"] = Value::Null;
        result.data["terminal_data_omitted"] = json!(true);
    }
    if detail_bytes(&result.data) > budget && result.data["terminal"]["error"].is_object() {
        result.data["terminal"]["error"]["message"] = json!("");
        result.data["terminal_error_omitted"] = json!(true);
    }
    while detail_bytes(&result.data) > budget {
        let stderr = result.data["diagnostics"]["stderr"].as_str().unwrap();
        if stderr.is_empty() {
            break;
        }
        let mut end = stderr.len() / 2;
        while !stderr.is_char_boundary(end) {
            end -= 1;
        }
        result.data["diagnostics"]["stderr"] = json!(&stderr[..end]);
        result.data["diagnostics"]["truncated"] = json!(true);
    }
    result.data["detail_bytes"] = json!(detail_bytes(&result.data));
    result.data["detail_budget_bytes"] = json!(budget);
    if result.data["omitted_progress"] != 0
        || result.data["terminal_data_omitted"] == true
        || result.data["terminal_error_omitted"] == true
        || result.data["diagnostics"]["truncated"] == true
    {
        if result.execution.status == Status::Succeeded {
            result.execution.status = Status::Partial;
        }
        result.coverage.state = "partial".into();
        result.coverage.limitations.push("Untrusted extension detail was omitted or truncated to honor the declared detail budget; identity and child outcome are retained.".into());
    }
}
pub(super) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    run_bound(request, control, context, start, result, None);
}
pub(super) fn run_bound(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
    expected: Option<&Value>,
) {
    result.data = envelope();
    result.data["detail_budget_bytes"] = json!(result.effective_limits.max_output_bytes);
    result.coverage.scope="One explicitly authorized external executable and its bounded protocol receipt; no live service or health claim.".into();
    result.coverage.limitations.push("operator-reviewed-v1 uses ordinary invoking-account authority. This is not an untrusted-code sandbox; declared observe effects and supplied provenance are not verified.".into());
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    if let Err(fault) = perform(request, control, context, deadline, result, expected) {
        failure(fault, control, result);
    }
    bound(result);
    if matches!(result.execution.status, Status::Succeeded | Status::Partial)
        && let Err(fault) = active(control, deadline)
    {
        failure(fault, control, result);
    }
}
fn select(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    deadline: Instant,
) -> Result<(package::Package, Value, Value), Fault> {
    active(control, deadline)?;
    if !cfg!(all(target_arch = "x86_64", target_pointer_width = "64")) {
        return Err(Fault::new("extension_unsupported_platform"));
    }
    let policy = policy::Policy::load(context, control, deadline)?
        .ok_or_else(|| Fault::new("extension_policy_required"))?;
    let path = context
        .extension_state_dir
        .as_deref()
        .ok_or_else(|| Fault::new("extension_store_required"))?;
    let mut store = storage::Store::open(path, false, control, deadline)?;
    let counts = policy::counts(&store.packages, Some(&policy));
    match counts.get(&request.operation).copied().unwrap_or(0) {
        0 => return Err(Fault::new("extension_not_authorized")),
        1 => {}
        _ => return Err(Fault::new("extension_ambiguous_operation")),
    }
    let index = store
        .packages
        .iter()
        .position(|p| {
            policy.grants(p, &request.operation)
                && p.manifest["capabilities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["operation"] == request.operation)
        })
        .unwrap();
    let package = store.packages.remove(index);
    drop(store);
    let capability = package.manifest["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["operation"] == request.operation)
        .unwrap()
        .clone();
    if !package::validate_values(&capability["input_schema"], &request.inputs) {
        return Err(Fault::new("invalid_extension_arguments"));
    }
    let identity = json!({"kind":"extension","policy_sha256":policy.digest,"package_sha256":package.digest,"executable_sha256":package.manifest["executable"]["sha256"],"operation":request.operation,"operation_version":1});
    Ok((package, capability, identity))
}
pub(super) fn job_binding(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    deadline: Instant,
) -> Result<Value, Fault> {
    Ok(select(request, control, context, deadline)?.2)
}
fn perform(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    deadline: Instant,
    result: &mut OperationResult,
    expected: Option<&Value>,
) -> Result<(), Fault> {
    let (package, capability, identity) = select(request, control, context, deadline)?;
    result.data["authorization"] = json!({"policy_sha256":identity["policy_sha256"],"execution_profile":"operator-reviewed-v1"});
    result.data["package"] = json!({"id":package.manifest["id"],"version":package.manifest["version"],"package_sha256":package.digest,"executable_sha256":package.manifest["executable"]["sha256"]});
    result.data["capability"] = json!({"operation":request.operation,"version":1});
    if expected.is_some_and(|expected| expected != &identity) {
        return Err(Fault::new("job_binding_changed"));
    }
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .min(request.limits.timeout_ms as u128) as u64;
    if remaining == 0 {
        return Err(Fault::new("extension_deadline_exceeded"));
    }
    let mut dialogue = protocol::Protocol::new(
        request,
        result,
        capability["output_schema"].clone(),
        remaining,
    )?;
    let bytes = package::executable_bytes(&package);
    active(control, deadline)?;
    let executable = snapshot(&bytes, control, deadline)?;
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .min(request.limits.timeout_ms as u128) as u64;
    if remaining == 0 {
        return Err(Fault::new("extension_deadline_exceeded"));
    }
    result.effective_limits.timeout_ms = remaining;
    result.resolved_target = json!({"profile":"operator-reviewed-v1","effects":"unclassified","package_sha256":package.digest,"executable_sha256":package.manifest["executable"]["sha256"],"resolution":"verified installed bytes copied to an unlinked readonly executable descriptor","cwd":"/"});
    let prepared = PreparedProcess {
        input: ProcessInput {
            program: "save-extension".into(),
            args: vec![],
            cwd: "/".into(),
        },
        executable: PathBuf::from("/not-used-extension-descriptor"),
        cwd: PathBuf::from("/"),
        descriptor_launch: Some(DescriptorLaunch {
            argv0: "save-extension",
            executable,
            inherited: vec![],
        }),
    };
    let saved = std::mem::replace(&mut result.data, json!({}));
    process::run_dialogue(prepared, result, control, &mut dialogue);
    let supervisor = std::mem::replace(&mut result.data, saved);
    let started = supervisor.get("process_id").is_some();
    if started {
        result.data["cancellation_signal"] = supervisor["cancellation_signal"].clone();
        result.data["supervisor"] = supervisor;
        result.data["protocol"] = dialogue.metadata();
        result.data["progress"] = json!(dialogue.progress);
        result.data["terminal"] = json!(dialogue.terminal);
    }
    result.data["diagnostics"] = json!({"stderr":std::mem::take(&mut result.output.stderr),"untrusted":true,"truncated":result.output.stderr_truncated||result.output.encoding!="utf-8"});
    result.output.stdout.clear();
    result.output.stdout_truncated = false;
    result.output.stderr_truncated = false;
    result.output.encoding = "utf-8".into();
    if started
        && !matches!(
            result.execution.status,
            Status::Cancelled | Status::TimedOut | Status::Unknown
        )
    {
        if !dialogue.complete() {
            result.execution.status = Status::Failed;
            result.coverage.state = "partial".into();
            if !result
                .errors
                .iter()
                .any(|e| e.code == "extension_protocol_error")
            {
                result.error("extension_protocol_error","The external peer did not complete exactly one valid protocol terminal and clean EOF.");
            }
        } else if result.data["terminal"]["status"] == "failed" {
            result.execution.status = Status::Failed;
            result.error(
                "extension_reported_failure",
                "The external peer reported failure; its untrusted detail is retained separately.",
            );
        } else if (result.execution.child_exit_code != Some(0)
            || result.data["supervisor"]["cleanup"]["confirmed"] != true)
            && result.execution.status == Status::Succeeded
        {
            result.execution.status = Status::Failed;
        }
    }
    result.coverage.scope="One explicitly authorized external executable and its bounded protocol receipt; no live service or health claim.".into();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_retains_exact_memory_bytes_without_names_or_writable_handles() {
        use std::io::Read;
        let mut bytes = b"already verified executable bytes".to_vec();
        let mut file = snapshot(
            &bytes,
            &RunControl::default(),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
        bytes.fill(b'x');
        let mut retained = Vec::new();
        file.read_to_end(&mut retained).unwrap();
        assert_eq!(retained, b"already verified executable bytes");
        assert_eq!(file.metadata().unwrap().nlink(), 0);
        assert_eq!(file.metadata().unwrap().mode() & 0o777, 0o500);
        assert_eq!(
            file.write_all(b"x").unwrap_err().raw_os_error(),
            Some(libc::EBADF)
        );
        let path = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).unwrap();
        assert!(path.to_string_lossy().ends_with("/executable (deleted)"));
        assert!(!path.parent().unwrap().exists());
    }
    #[test]
    fn detail_budget_preserves_identity_child_outcomes_and_explicit_omissions() {
        let mut request = Request::new("ext.fixture.run", json!({}));
        request.limits.max_output_bytes = 1024;
        let mut result = OperationResult::new(&request);
        result.data = envelope();
        result.effect_outcome = "unknown".into();
        result.data["package"] = json!({"package_sha256":"pinned"});
        result.data["supervisor"] = json!({"process_id":123,"cleanup":{"confirmed":true}});
        result.data["terminal"] = json!({"sequence":2,"status":"succeeded","data":{"value":"x".repeat(4096)},"error":null,"untrusted":true});
        result.data["progress"] =
            json!([{"sequence":1,"message":"x".repeat(1024),"untrusted":true}]);
        result.data["diagnostics"]["stderr"] = json!("\u{1}".repeat(65536));
        bound(&mut result);
        assert_eq!(result.execution.status, Status::Partial);
        assert_eq!(result.effect_outcome, "unknown");
        assert_eq!(result.data["package"]["package_sha256"], "pinned");
        assert_eq!(result.data["supervisor"]["process_id"], 123);
        assert_eq!(result.data["omitted_progress"], 1);
        assert_eq!(result.data["terminal_data_omitted"], true);
        assert_eq!(result.data["terminal"]["status"], "succeeded");
        assert!(detail_bytes(&result.data) <= 1024);
        assert_eq!(result.data["detail_bytes"], detail_bytes(&result.data));
        result.execution.status = Status::Failed;
        result.data["terminal"] = json!({"sequence":1,"status":"failed","data":null,"error":{"code":"peer_failed","message":"\u{1}".repeat(1024)},"untrusted":true});
        bound(&mut result);
        assert_eq!(result.execution.status, Status::Failed);
        assert_eq!(result.data["terminal_error_omitted"], true);
        assert_eq!(result.data["terminal"]["error"]["code"], "peer_failed");
        assert_eq!(result.data["terminal"]["error"]["message"], "");
        assert!(detail_bytes(&result.data) <= 1024);
    }
}
