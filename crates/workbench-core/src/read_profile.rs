//! Explicit launcher-owned Linux read policy; no request can select this authority.
pub(crate) mod grammar;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod policy;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) mod seccomp;

use crate::{RunControl, request::Request, result::OperationResult};
use std::{path::Path, time::Instant};

/// Read-only description from the same trusted policy loader used by execution.
pub struct ReadPolicyRoot {
    pub id: String,
    pub path: String,
}

pub struct ReadPolicyDescription {
    pub roots: Vec<ReadPolicyRoot>,
    pub digest: String,
}

pub fn read_policy_description(
    path: &Path,
) -> Result<ReadPolicyDescription, crate::request::Problem> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let (policy, digest) = policy::load(path)?;
        Ok(ReadPolicyDescription {
            roots: policy
                .roots
                .into_iter()
                .map(|root| ReadPolicyRoot {
                    id: root.id,
                    path: root.path,
                })
                .collect(),
            digest,
        })
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        let _ = path;
        Err(crate::request::Problem::unsupported(
            "unsupported_read_profile",
            "The trusted read policy requires x86_64 Linux.",
        ))
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub(crate) fn run_with_policy_identity(
    _: &Request,
    _: &RunControl,
    _: &Path,
    _: Option<&crate::ReadProfileLauncher>,
    _: Option<&str>,
    _: Instant,
    result: &mut OperationResult,
) {
    result.reject(crate::request::Problem::unsupported(
        "unsupported_read_profile",
        "linux-read-v1 currently requires the verified x86_64 GNU/Linux runtime layout.",
    ));
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) use linux::run_with_policy_identity;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux {
    use super::*;
    use crate::{
        process::{self, DescriptorLaunch, PreparedProcess},
        request::{Problem, ProcessInput, parse_bounded_json},
        result::Status,
    };
    use serde_json::{Value, json};
    use std::{
        fs::File,
        io::{Read, Seek, SeekFrom, Write},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::MetadataExt,
        },
        path::PathBuf,
        sync::atomic::Ordering,
        time::Duration,
    };

    const TMP_BYTES: usize = 16 * 1024 * 1024;
    struct Prepared {
        process: PreparedProcess,
        status: File,
        launcher_status: File,
        data: Value,
    }

    fn above_stdio(file: File) -> Result<File, Problem> {
        // SAFETY: the live file is duplicated above stdio without changing its ownership.
        let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        if fd < 0 {
            return Err(policy::error("read_descriptor_unavailable"));
        }
        // SAFETY: successful fcntl returned this distinct owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn sealed(bytes: &[u8]) -> Result<File, Problem> {
        // SAFETY: the constant name is NUL-terminated; memfd returns a new owned fd.
        let fd = unsafe {
            libc::memfd_create(
                c"read-profile".as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
            )
        };
        if fd < 0 {
            return Err(policy::error("read_descriptor_unavailable"));
        }
        // SAFETY: successful memfd_create transfers this unique descriptor.
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(bytes)
            .and_then(|_| file.seek(SeekFrom::Start(0)))
            .map_err(|_| policy::error("read_descriptor_unavailable"))?;
        // SAFETY: fcntl operates on our live memfd and does not retain references.
        if unsafe {
            libc::fcntl(
                fd,
                libc::F_ADD_SEALS,
                libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL,
            )
        } < 0
        {
            return Err(policy::error("read_descriptor_unavailable"));
        }
        above_stdio(file)
    }

    fn status_pipe() -> Result<(File, File), Problem> {
        let mut fds = [-1; 2];
        // SAFETY: fds has room for both descriptors returned by pipe2.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
            return Err(policy::error("read_descriptor_unavailable"));
        }
        // SAFETY: pipe2 returned two distinct owned descriptors.
        let reader = unsafe { File::from_raw_fd(fds[0]) };
        // SAFETY: pipe2 returned two distinct owned descriptors.
        let writer = unsafe { File::from_raw_fd(fds[1]) };
        // The trusted wrapper writes two small records. A broken binding cannot
        // make its status channel block or allocate unbounded reader storage.
        // SAFETY: F_SETPIPE_SZ adjusts the live owned pipe only.
        if unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_SETPIPE_SZ, 4096) } < 0 {
            return Err(policy::error("read_descriptor_unavailable"));
        }
        Ok((above_stdio(reader)?, above_stdio(writer)?))
    }

    fn bind(args: &mut Vec<String>, files: &mut Vec<File>, file: File, destination: &str) {
        args.extend([
            "--ro-bind-fd".into(),
            file.as_raw_fd().to_string(),
            destination.into(),
        ]);
        files.push(file);
    }
    fn environment(args: &mut Vec<String>, name: &str, value: &str) {
        args.extend(["--setenv".into(), name.into(), value.into()]);
    }

    #[cfg(test)]
    fn prepare(
        request: &Request,
        control: &RunControl,
        path: &Path,
        image: Option<&crate::ReadProfileLauncher>,
        deadline: Instant,
    ) -> Result<Prepared, Problem> {
        prepare_with_identity(request, control, path, image, None, deadline)
    }

    fn prepare_with_identity(
        request: &Request,
        control: &RunControl,
        path: &Path,
        image: Option<&crate::ReadProfileLauncher>,
        expected_policy_digest: Option<&str>,
        deadline: Instant,
    ) -> Result<Prepared, Problem> {
        policy::check(control, deadline)?;
        let input = process::validate_input(&request.inputs)?;
        let command = grammar::normalize(&input)?;
        let (policy, sha256) = policy::load(path)?;
        if expected_policy_digest.is_some_and(|expected| expected != sha256) {
            return Err(policy::error("read_policy_changed"));
        }
        let root = policy
            .roots
            .iter()
            .find(|root| root.path == input.cwd)
            .ok_or_else(|| policy::error("read_root_denied"))?;
        let root_file = policy::open_at(
            libc::AT_FDCWD,
            &root.path,
            libc::O_PATH | libc::O_DIRECTORY,
            false,
        )
        .map_err(|_| policy::error("read_root_unavailable"))?;
        let metadata = root_file
            .metadata()
            .map_err(|_| policy::error("read_root_unavailable"))?;
        for path in &command.paths {
            match policy::open_at(root_file.as_raw_fd(), path, libc::O_PATH, true) {
                Ok(_) => {}
                Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {}
                Err(_) => return Err(policy::error("read_path_denied")),
            }
        }
        let git = if command.name == "git" {
            Some(policy::repository(&root_file)?)
        } else {
            None
        };
        let selected = if command.name == "git" {
            &policy.executables.git
        } else {
            &policy.executables.rg
        };
        let tool = policy::pin(&selected.path, Some(&selected.sha256), control, deadline)?;
        let wrapper = policy::pin(
            &policy.executables.bwrap.path,
            Some(&policy.executables.bwrap.sha256),
            control,
            deadline,
        )?;
        let image = image.ok_or_else(|| {
            Problem::unsupported(
                "read_launcher_unavailable",
                "The trusted host must supply the fixed current-image launcher entry.",
            )
        })?;
        if image
            .image
            .metadata()
            .map_err(|_| policy::error("read_binding_unavailable"))?
            .len()
            > policy::MAX_FILE
        {
            return Err(Problem::unsupported(
                "unsupported_launcher_image",
                "The current launcher image exceeds 64MiB. Use a release or debug-symbol-free build outside the granted root.",
            ));
        }
        let image_path = image
            .path
            .to_str()
            .filter(|path| policy::absolute(path))
            .ok_or_else(|| policy::error("read_launcher_unavailable"))?;
        if policy
            .roots
            .iter()
            .any(|root| image.path.starts_with(&root.path))
        {
            return Err(policy::error("read_launcher_inside_root"));
        }
        let image_file = above_stdio(
            image
                .image
                .try_clone()
                .map_err(|_| policy::error("read_descriptor_unavailable"))?,
        )?;
        let launcher = policy::pin_file(image_file, image_path, None, control, deadline)?;
        let mut args: Vec<String> = [
            "--unshare-user",
            "--unshare-net",
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--die-with-parent",
            "--new-session",
            "--cap-drop",
            "ALL",
            "--clearenv",
        ]
        .map(str::to_owned)
        .into();
        let mut files = Vec::new();
        bind(&mut args, &mut files, root_file, "/workspace");
        bind(&mut args, &mut files, tool.file, "/tool");
        bind(&mut args, &mut files, launcher.file, "/launcher");
        let mut runtime = Vec::new();
        for destination in policy::RUNTIME {
            let canonical = std::fs::canonicalize(destination).map_err(|_| {
                Problem::unsupported(
                    "unsupported_read_runtime",
                    "The fixed x86_64 GNU/Linux runtime files are unavailable.",
                )
            })?;
            let canonical = canonical
                .to_str()
                .ok_or_else(|| policy::error("read_binding_unavailable"))?;
            if policy
                .roots
                .iter()
                .any(|root| Path::new(canonical).starts_with(&root.path))
            {
                return Err(policy::error("invalid_read_policy"));
            }
            let mut pinned = policy::pin(canonical, None, control, deadline)?;
            pinned.identity["path"] = json!(destination);
            runtime.push(pinned.identity);
            bind(&mut args, &mut files, pinned.file, destination);
        }
        if let Some(git) = git {
            bind(&mut args, &mut files, git, "/git");
            let empty = sealed(&[])?;
            args.extend([
                "--ro-bind-data".into(),
                empty.as_raw_fd().to_string(),
                "/git/config".into(),
            ]);
            files.push(empty);
        }
        args.extend(
            [
                "--dev",
                "/dev",
                "--remount-ro",
                "/dev",
                "--size",
                &TMP_BYTES.to_string(),
                "--tmpfs",
                "/tmp",
                "--remount-ro",
                "/",
                "--chdir",
                "/workspace",
            ]
            .map(str::to_owned),
        );
        for (key, value) in [
            ("PATH", "/nonexistent"),
            ("LANG", "C.UTF-8"),
            ("LC_ALL", "C.UTF-8"),
            ("TZ", "UTC"),
            ("HOME", "/nonexistent"),
            ("TMPDIR", "/tmp"),
        ] {
            environment(&mut args, key, value);
        }
        if command.name == "git" {
            for (key, value) in [
                ("GIT_DIR", "/git"),
                ("GIT_WORK_TREE", "/workspace"),
                ("GIT_CONFIG_GLOBAL", "/dev/null"),
                ("GIT_CONFIG_SYSTEM", "/dev/null"),
                ("GIT_CONFIG_NOSYSTEM", "1"),
                ("GIT_OPTIONAL_LOCKS", "0"),
                ("GIT_NO_LAZY_FETCH", "1"),
                ("GIT_NO_REPLACE_OBJECTS", "1"),
                ("GIT_ATTR_NOSYSTEM", "1"),
                ("GIT_TERMINAL_PROMPT", "0"),
            ] {
                environment(&mut args, key, value);
            }
        }
        let filter = sealed(&seccomp::program())?;
        args.extend(["--seccomp".into(), filter.as_raw_fd().to_string()]);
        files.push(filter);
        let (status, writer) = status_pipe()?;
        args.extend(["--json-status-fd".into(), writer.as_raw_fd().to_string()]);
        files.push(writer);
        let encoded_metadata = serde_json::to_vec(&json!({"version":1,"input":request.inputs,"root_device":metadata.dev(),"root_inode":metadata.ino(),"tool_device":tool.identity["device"],"tool_inode":tool.identity["inode"]})).expect("fixed launch metadata");
        if encoded_metadata.len() > crate::read_launcher::MAX_METADATA_BYTES {
            return Err(policy::error("read_launch_metadata_too_large"));
        }
        let launch_metadata = sealed(&encoded_metadata)?;
        let (launcher_status, launcher_writer) = status_pipe()?;
        args.extend([
            "--".into(),
            "/launcher".into(),
            crate::read_launcher::ENTRY.into(),
            launch_metadata.as_raw_fd().to_string(),
            launcher_writer.as_raw_fd().to_string(),
        ]);
        files.extend([launch_metadata, launcher_writer]);
        let data = json!({
            "profile":"linux-read-v1","effects":"observe","execution_performed":false,"inspection_advisory":request.operation == "command.inspect",
            "policy":{"sha256":sha256},"root":{"id":root.id,"path":root.path,"device":metadata.dev(),"inode":metadata.ino()},
            "tool":{"name":command.name,"binding":tool.identity,"normalized_args":command.args,"execution_confirmed":false,"exit_status":null,"status_encoding":"exit_or_128_plus_signal"},
            "wrapper":{"binding":wrapper.identity,"exit_code":null,"signal":null},"runtime":runtime,
            "launcher":{"binding":launcher.identity,"state":"required","landlock_abi":null},
            "isolation":{"state":"required","namespaces":["user","mount","network","pid","ipc","uts"],"root_read_only":true,"proc_mounted":false,"tmp_bytes":TMP_BYTES,"no_new_privileges":true,"capabilities":"none","seccomp":"x86_64-read-v1","landlock_minimum_abi":5,"writable_file_opens":"private_dev_null_only","socket_syscalls":false},
            "supervisor":null,"cancellation_signal":null
        });
        policy::check(control, deadline)?;
        Ok(Prepared {
            process: PreparedProcess {
                input: ProcessInput {
                    program: policy.executables.bwrap.path.clone(),
                    args,
                    cwd: "/".into(),
                },
                executable: PathBuf::from(&policy.executables.bwrap.path),
                cwd: PathBuf::from("/"),
                descriptor_launch: Some(DescriptorLaunch {
                    executable: wrapper.file,
                    inherited: files,
                }),
            },
            status,
            launcher_status,
            data,
        })
    }

    fn channel_bytes(mut file: File) -> Result<Vec<u8>, ()> {
        let mut bytes = Vec::new();
        match (&mut file).take(4097).read_to_end(&mut bytes) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => return Err(()),
        }
        if bytes.len() > 4096 || (!bytes.is_empty() && !bytes.ends_with(b"\n")) {
            return Err(());
        }
        Ok(bytes)
    }

    fn status(file: File) -> Result<Option<u64>, ()> {
        let bytes = channel_bytes(file)?;
        let mut child = false;
        let mut exit = None;
        let mut count = 0;
        for line in bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()) {
            count += 1;
            if count > 2 {
                return Err(());
            }
            let value = parse_bounded_json(line, 4).map_err(|_| ())?;
            let object = value.as_object().ok_or(())?;
            if object.len() > 16 {
                return Err(());
            }
            if let Some(pid) = value.get("child-pid") {
                if child
                    || exit.is_some()
                    || !pid.as_u64().is_some_and(|p| p > 0 && p <= i32::MAX as u64)
                {
                    return Err(());
                }
                child = true;
            } else if let Some(code) = value.get("exit-code") {
                if !child || exit.is_some() {
                    return Err(());
                }
                exit = Some(code.as_u64().filter(|c| *c <= 255).ok_or(())?);
            } else {
                return Err(());
            }
        }
        Ok(exit)
    }

    #[derive(serde::Deserialize)]
    #[serde(tag = "stage", rename_all = "snake_case", deny_unknown_fields)]
    enum LauncherRecord {
        Admitted { abi: u64 },
        ToolStarted,
        ToolExit { code: u8 },
        SetupFailed { code: String },
        ExecFailed { code: String },
    }
    struct LauncherReport {
        state: &'static str,
        abi: Option<u64>,
        exit: Option<u64>,
        error: Option<&'static str>,
    }
    fn launcher_status(file: File) -> Result<LauncherReport, ()> {
        let bytes = channel_bytes(file)?;
        let mut report = LauncherReport {
            state: "unconfirmed",
            abi: None,
            exit: None,
            error: None,
        };
        let mut count = 0;
        for line in bytes.split(|b| *b == b'\n').filter(|v| !v.is_empty()) {
            count += 1;
            if count > 3 {
                return Err(());
            }
            let value = parse_bounded_json(line, 4).map_err(|_| ())?;
            if !value.is_object() {
                return Err(());
            }
            let record: LauncherRecord = serde_json::from_value(value).map_err(|_| ())?;
            match (report.state, record) {
                ("unconfirmed", LauncherRecord::Admitted { abi }) if (5..=1024).contains(&abi) => {
                    report.state = "admitted";
                    report.abi = Some(abi);
                }
                ("admitted", LauncherRecord::ToolStarted) => report.state = "tool_started",
                ("tool_started", LauncherRecord::ToolExit { code }) => {
                    report.state = "tool_exited";
                    report.exit = Some(u64::from(code));
                }
                ("unconfirmed", LauncherRecord::SetupFailed { code }) => {
                    report.state = "setup_failed";
                    report.error = Some(match code.as_str() {
                        "launcher_admission_failed" => "launcher_admission_failed",
                        "landlock_unavailable" => "landlock_unavailable",
                        _ => return Err(()),
                    });
                }
                ("admitted", LauncherRecord::ExecFailed { code }) if code == "tool_exec_failed" => {
                    report.state = "exec_failed";
                    report.error = Some("tool_exec_failed");
                }
                _ => return Err(()),
            }
        }
        Ok(report)
    }

    pub(crate) fn run_with_policy_identity(
        request: &Request,
        control: &RunControl,
        path: &Path,
        image: Option<&crate::ReadProfileLauncher>,
        expected_policy_digest: Option<&str>,
        start: Instant,
        result: &mut OperationResult,
    ) {
        let deadline = start + Duration::from_millis(result.effective_limits.timeout_ms);
        let prepared = match prepare_with_identity(
            request,
            control,
            path,
            image,
            expected_policy_digest,
            deadline,
        ) {
            Ok(prepared) => prepared,
            Err(problem) => {
                let code = problem.code;
                result.reject(problem);
                if matches!(code, "cancelled" | "timed_out") {
                    result.execution.status = if code == "cancelled" {
                        Status::Cancelled
                    } else {
                        Status::TimedOut
                    };
                    result.data =
                        json!({"cancellation_signal":control.signal.load(Ordering::Relaxed)});
                }
                return;
            }
        };
        result.resolved_target = json!({"profile":"linux-read-v1","effects":"observe","cwd":request.inputs["cwd"],"executable":prepared.data["tool"]["binding"]["path"],"root_id":prepared.data["root"]["id"]});
        result.coverage.scope =
            "Output from one allowed observation of the granted working tree".into();
        result.coverage.limitations = vec![
            "Required namespace and Landlock controls remain unconfirmed without complete trusted launcher and wrapper receipts. Inspection launches nothing and does not establish kernel availability. Only the fixed private /dev/null device permits writable opens.".into(),
            "Only the selected root, pinned tool/launcher images and six declared runtime files are readable. Sensitive files intentionally granted in that root remain readable; no protected same-account identity or defense against kernel defects is claimed.".into(),
            "Git configuration is masked, untracked/submodule inspection is omitted, and linked/sparse/partial/shallow repositories are unsupported. Ripgrep omits .git, limits files to 8MiB and matches to 1000 per file.".into(),
            "The outer supervisor reports wrapper exit and process-group cleanup; the tool status uses Bubblewrap's exit-or-128-plus-signal encoding. Client cancellation does not establish which instruction was last executed.".into(),
            "Time, captured output and private temporary storage are bounded. This profile has no per-invocation total-memory or process-count quota.".into(),
        ];
        if request.operation == "command.inspect" {
            result.data = prepared.data;
            result.effect_outcome = "not_applicable".into();
            return;
        }
        let limits = result.coverage.limitations.clone();
        let scope = result.coverage.scope.clone();
        let timeout = result.effective_limits.timeout_ms;
        result.effective_limits.timeout_ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .max(1) as u64;
        process::run(prepared.process, result, control);
        result.effective_limits.timeout_ms = timeout;
        let mut supervisor = std::mem::replace(&mut result.data, prepared.data);
        if let Some(object) = supervisor.as_object_mut() {
            object.remove("profile");
            object.remove("effects");
        }
        result.data["execution_performed"] = json!(supervisor.get("process_id").is_some());
        result.data["cancellation_signal"] = supervisor
            .get("cancellation_signal")
            .cloned()
            .unwrap_or(Value::Null);
        result.data["wrapper"]["exit_code"] = json!(result.execution.child_exit_code);
        result.data["wrapper"]["signal"] = json!(result.execution.signal);
        result.data["supervisor"] = if supervisor.get("process_id").is_some() {
            supervisor
        } else {
            Value::Null
        };
        result.coverage.scope = scope;
        result.coverage.limitations = limits;
        if result.effect_outcome != "not_attempted" {
            result.effect_outcome = "not_applicable".into();
        }
        let reported = status(prepared.status);
        let launch = launcher_status(prepared.launcher_status);
        if let Ok(report) = &launch {
            result.data["launcher"]["state"] = json!(report.state);
            result.data["launcher"]["landlock_abi"] = json!(report.abi);
        } else {
            result.data["launcher"]["state"] = json!("unconfirmed");
        }
        match (&reported, &launch) {
            (Ok(Some(code)), Ok(launcher))
                if launcher.exit == Some(*code)
                    && result
                        .execution
                        .child_exit_code
                        .is_some_and(|wrapper| u64::try_from(wrapper).ok() == Some(*code)) =>
            {
                result.data["tool"]["execution_confirmed"] = json!(true);
                result.data["tool"]["exit_status"] = json!(code);
                result.data["isolation"]["state"] = json!("confirmed");
            }
            _ => {
                result.data["isolation"]["state"] = json!("unconfirmed");
                result.coverage.state = "not_established".into();
                if matches!(
                    result.execution.status,
                    Status::Succeeded | Status::Failed | Status::Partial
                ) {
                    result.execution.status = Status::Failed;
                }
                let code = match (&reported, &launch) {
                    (Err(()), _) => "sandbox_status_invalid",
                    (_, Err(())) => "launcher_status_invalid",
                    (_, Ok(report)) if report.error.is_some() => {
                        report.error.expect("guarded error")
                    }
                    (Ok(None), Ok(report)) if report.state == "unconfirmed" => {
                        "sandbox_not_established"
                    }
                    _ => "launcher_unconfirmed",
                };
                result.error(code, "The trusted launcher and wrapper did not establish a complete tool-exec receipt. Required isolation was not relaxed and no fallback was attempted.");
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        fn native_plan(args: &[&str]) -> (Scratch, Prepared, Request) {
            assert_eq!(
                std::env::var("WORKBENCH_PROFILE_REQUIRED").as_deref(),
                Ok("1"),
                "requires the native Cally boundary"
            );
            let base = std::env::temp_dir().join(crate::result::new_id("read-profile-core"));
            std::fs::create_dir(&base).unwrap();
            std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
            let root = base.join("checkout");
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("original.txt"), "original needle\n").unwrap();
            let mut bindings = serde_json::Map::new();
            for name in ["git", "rg", "bwrap"] {
                let source =
                    std::env::var(format!("WORKBENCH_PROFILE_{}", name.to_ascii_uppercase()))
                        .expect("required native binding");
                let copied = base.join(name);
                std::fs::copy(source, &copied).unwrap();
                std::fs::set_permissions(&copied, std::fs::Permissions::from_mode(0o755)).unwrap();
                bindings.insert(name.into(), json!({"path":copied,"sha256":policy::digest(&std::fs::read(&copied).unwrap())}));
            }
            let path = base.join("policy.json");
            std::fs::write(&path, serde_json::to_vec(&json!({"version":1,"profile":"linux-read-v1","roots":[{"id":"checkout","path":root}],"executables":bindings})).unwrap()).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            let request = Request::new(
                "process.exec",
                json!({"program":"rg","args":args,"cwd":root}),
            );
            let binary = std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("save");
            let copied_image = base.join("save");
            std::fs::copy(binary, &copied_image).unwrap();
            std::fs::set_permissions(&copied_image, std::fs::Permissions::from_mode(0o755))
                .unwrap();
            let image = crate::ReadProfileLauncher {
                image: File::open(&copied_image).unwrap(),
                path: copied_image,
            };
            let prepared = prepare(
                &request,
                &RunControl::default(),
                &path,
                Some(&image),
                Instant::now() + Duration::from_secs(10),
            )
            .unwrap();
            (Scratch(base), prepared, request)
        }

        #[test]
        fn trusted_status_records_are_bounded_ordered_and_typed() {
            for (text, expected) in [
                ("", None),
                ("{\"child-pid\":12}\n", None),
                ("{\"child-pid\":12}\n{\"exit-code\":1}\n", Some(1)),
            ] {
                assert_eq!(status(sealed(text.as_bytes()).unwrap()), Ok(expected));
            }
            for text in [
                "{\"exit-code\":0}\n",
                "{\"child-pid\":1}\n{\"exit-code\":256}\n",
                "{\"child-pid\":1,\"child-pid\":2}\n",
                "[1]\n",
                "{\"child-pid\":1}",
                "{\"child-pid\":1}\n{\"child-pid\":2}\n",
            ] {
                assert!(status(sealed(text.as_bytes()).unwrap()).is_err(), "{text}");
            }
            assert!(status(sealed(&vec![b' '; 4097]).unwrap()).is_err());
        }

        #[test]
        fn launcher_protocol_requires_setup_start_and_exit_in_order() {
            let complete = "{\"stage\":\"admitted\",\"abi\":8}\n{\"stage\":\"tool_started\"}\n{\"stage\":\"tool_exit\",\"code\":1}\n";
            let report = launcher_status(sealed(complete.as_bytes()).unwrap())
                .ok()
                .unwrap();
            assert_eq!(report.exit, Some(1));
            assert_eq!(report.state, "tool_exited");
            assert_eq!(report.abi, Some(8));
            let partial = "{\"stage\":\"admitted\",\"abi\":8}\n{\"stage\":\"tool_started\"}\n";
            let report = launcher_status(sealed(partial.as_bytes()).unwrap())
                .ok()
                .unwrap();
            assert_eq!(report.state, "tool_started");
            assert_eq!(report.exit, None);
            let failed = "{\"stage\":\"admitted\",\"abi\":8}\n{\"stage\":\"exec_failed\",\"code\":\"tool_exec_failed\"}\n";
            let report = launcher_status(sealed(failed.as_bytes()).unwrap())
                .ok()
                .unwrap();
            assert_eq!(report.error, Some("tool_exec_failed"));
            assert_eq!(report.exit, None);
            for bad in [
                "{\"stage\":\"tool_exit\",\"code\":0}\n",
                "{\"stage\":\"admitted\",\"abi\":4}\n",
                "{\"stage\":\"admitted\",\"abi\":8,\"extra\":true}\n",
                "[\"admitted\",8]\n",
                "{\"stage\":\"setup_failed\",\"code\":\"caller_message\"}\n",
            ] {
                assert!(
                    launcher_status(sealed(bad.as_bytes()).unwrap()).is_err(),
                    "{bad}"
                );
            }
        }

        #[test]
        #[ignore = "requires native Cally Landlock admission"]
        fn native_landlock_setup_failure_never_executes_tool() {
            let (_scratch, mut prepared, request) = native_plan(&["--files"]);
            let mut filter = seccomp::program();
            // Preserve the actual filter and inject a denied Landlock-version
            // syscall before its final allow. The real launcher must fail closed.
            let terminal = filter.len() - 8;
            let mut deny = Vec::new();
            deny.extend_from_slice(&0x15_u16.to_le_bytes());
            deny.extend_from_slice(&[0, 1]);
            deny.extend_from_slice(&(libc::SYS_landlock_create_ruleset as u32).to_le_bytes());
            deny.extend_from_slice(&0x06_u16.to_le_bytes());
            deny.extend_from_slice(&[0, 0]);
            deny.extend_from_slice(&(0x0005_0000 | libc::EPERM as u32).to_le_bytes());
            filter.splice(terminal..terminal, deny);
            let fd = sealed(&filter).unwrap();
            let option = prepared
                .process
                .input
                .args
                .iter()
                .position(|v| v == "--seccomp")
                .unwrap()
                + 1;
            prepared.process.input.args[option] = fd.as_raw_fd().to_string();
            prepared
                .process
                .descriptor_launch
                .as_mut()
                .unwrap()
                .inherited
                .push(fd);
            let mut result = OperationResult::new(&request);
            process::run(prepared.process, &mut result, &RunControl::default());
            assert_eq!(
                status(prepared.status),
                Ok(Some(125)),
                "{}",
                result.json_bytes().escape_ascii()
            );
            let report = launcher_status(prepared.launcher_status).ok().unwrap();
            assert_eq!(report.state, "setup_failed");
            assert_eq!(report.error, Some("landlock_unavailable"));
            assert_eq!(report.exit, None);
            assert!(result.output.stdout.is_empty());
        }

        #[test]
        #[ignore = "requires native Cally trusted runtime identity"]
        fn native_oversized_launcher_image_is_explicitly_unsupported() {
            let (scratch, _prepared, request) = native_plan(&["--files"]);
            let path = scratch.0.join("save");
            std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(policy::MAX_FILE + 1)
                .unwrap();
            let image = crate::ReadProfileLauncher {
                image: File::open(&path).unwrap(),
                path,
            };
            let problem = prepare(
                &request,
                &RunControl::default(),
                &scratch.0.join("policy.json"),
                Some(&image),
                Instant::now() + Duration::from_secs(3),
            )
            .err()
            .unwrap();
            assert_eq!(problem.code, "unsupported_launcher_image");
            assert!(problem.unsupported);
        }

        #[test]
        #[ignore = "requires native Cally descriptor-pinned execution"]
        fn native_replaced_root_and_executable_names_do_not_replace_pinned_bindings() {
            let (scratch, prepared, request) = native_plan(&["--files"]);
            std::fs::rename(scratch.0.join("checkout"), scratch.0.join("original-root")).unwrap();
            std::fs::create_dir(scratch.0.join("checkout")).unwrap();
            std::fs::write(
                scratch.0.join("checkout/replacement-canary"),
                "must not be granted",
            )
            .unwrap();
            for name in ["rg", "bwrap", "save"] {
                std::fs::rename(
                    scratch.0.join(name),
                    scratch.0.join(format!("pinned-{name}")),
                )
                .unwrap();
                // A pathname-based launch would now execute false, which cannot
                // produce the original directory listing or a success receipt.
                std::fs::copy("/usr/bin/false", scratch.0.join(name)).unwrap();
                std::fs::set_permissions(
                    scratch.0.join(name),
                    std::fs::Permissions::from_mode(0o755),
                )
                .unwrap();
            }
            let mut result = OperationResult::new(&request);
            process::run(prepared.process, &mut result, &RunControl::default());
            assert_eq!(
                result.execution.status,
                Status::Succeeded,
                "{}",
                result.json_bytes().escape_ascii()
            );
            assert_eq!(status(prepared.status), Ok(Some(0)));
            assert!(result.output.stdout.contains("original.txt"));
            assert!(!result.output.stdout.contains("replacement-canary"));
        }

        #[test]
        #[ignore = "requires native Cally filesystem containment"]
        fn native_explicit_path_raced_to_outside_symlink_cannot_read_outside() {
            let (scratch, prepared, request) = native_plan(&["-e", "needle", "--", "original.txt"]);
            std::fs::write(scratch.0.join("outside"), "outside needle canary").unwrap();
            std::fs::remove_file(scratch.0.join("checkout/original.txt")).unwrap();
            std::os::unix::fs::symlink(
                scratch.0.join("outside"),
                scratch.0.join("checkout/original.txt"),
            )
            .unwrap();
            let mut result = OperationResult::new(&request);
            process::run(prepared.process, &mut result, &RunControl::default());
            assert_eq!(
                status(prepared.status),
                Ok(Some(2)),
                "{}",
                result.json_bytes().escape_ascii()
            );
            assert!(!result.output.stdout.contains("outside needle canary"));
            assert_eq!(result.execution.child_exit_code, Some(2));
        }
    }
}
