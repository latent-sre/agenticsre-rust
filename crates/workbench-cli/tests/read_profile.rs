#![cfg(all(target_os = "linux", target_arch = "x86_64", feature = "test-fixtures"))]

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{OpenOptionsExt, PermissionsExt},
            net::UnixListener,
        },
    },
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn digest(path: &Path) -> String {
    let bytes = Sha256::digest(fs::read(path).unwrap());
    let mut value = "sha256:".to_owned();
    for b in bytes {
        use std::fmt::Write;
        write!(value, "{b:02x}").unwrap();
    }
    value
}
fn tool(name: &str) -> PathBuf {
    let key = format!("WORKBENCH_PROFILE_{}", name.to_ascii_uppercase());
    fs::canonicalize(
        std::env::var_os(key)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("/usr/bin/{name}"))),
    )
    .unwrap()
}
fn required() {
    assert_eq!(
        std::env::var("WORKBENCH_PROFILE_REQUIRED").as_deref(),
        Ok("1"),
        "native proof requires its explicitly configured isolation lane"
    );
    for name in ["BWRAP", "GIT", "RG"] {
        let path = std::env::var(format!("WORKBENCH_PROFILE_{name}"))
            .expect("required trusted test binding");
        assert!(Path::new(&path).is_absolute() && Path::new(&path).is_file());
    }
}

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    policy: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "read-profile-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&base).unwrap();
        fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
        let root = base.join("checkout");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("read.txt"), "synthetic needle λ\n").unwrap();
        let policy = base.join("policy.json");
        let mut bindings = serde_json::Map::new();
        for name in ["git", "rg", "bwrap"] {
            let path = base.join(name);
            fs::copy(tool(name), &path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            bindings.insert(name.into(), json!({"path":path,"sha256":digest(&path)}));
        }
        fs::write(&policy,serde_json::to_vec(&json!({"version":1,"profile":"linux-read-v1","roots":[{"id":"checkout","path":root}],"executables":bindings})).unwrap()).unwrap();
        fs::set_permissions(&policy, fs::Permissions::from_mode(0o600)).unwrap();
        let fixture = Self { base, root, policy };
        fixture.git(&["init", "--quiet", "--template="]);
        fixture.git(&["add", "read.txt"]);
        fixture.git(&["commit", "--quiet", "-m", "fixture baseline"]);
        fixture
    }
    fn git(&self, args: &[&str]) {
        let output = Command::new(tool("git"))
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git fixture setup: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fn command(&self, operation: &str, argv: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_save"));
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["--json", "--read-policy"])
            .arg(&self.policy);
        if operation == "inspect" {
            command.args(["command", "inspect"]);
        } else {
            command.arg("exec");
        }
        command.arg("--cwd").arg(&self.root).arg("--").args(argv);
        command
    }
    fn run(&self, operation: &str, argv: &[&str]) -> (Output, Value) {
        run(self.command(operation, argv))
    }
    fn edit(&self, edit: impl FnOnce(&mut Value)) {
        let mut policy: Value = serde_json::from_slice(&fs::read(&self.policy).unwrap()).unwrap();
        edit(&mut policy);
        fs::write(&self.policy, serde_json::to_vec(&policy).unwrap()).unwrap();
    }
    fn fixture_binding(&self, mode: Value) {
        fs::write(
            self.root.join("mode.json"),
            serde_json::to_vec(&mode).unwrap(),
        )
        .unwrap();
        let path = PathBuf::from(env!("CARGO_BIN_EXE_profile-fixture"));
        self.edit(|p| p["executables"]["rg"] = json!({"path":path,"sha256":digest(&path)}));
    }
    fn with_limits(&self, timeout: &str, maximum: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_save"));
        command
            .env_clear()
            .args(["--json", "--read-policy"])
            .arg(&self.policy)
            .args(["exec", "--cwd"])
            .arg(&self.root)
            .args([
                "--timeout",
                timeout,
                "--max-output-bytes",
                maximum,
                "--",
                "rg",
                "--files",
            ]);
        command
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}
fn run(mut command: Command) -> (Output, Value) {
    let output = command.stdin(Stdio::null()).output().unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "invalid receipt {e}: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert!(output.stdout.len() <= 2_097_152);
    (output, value)
}
fn has(value: &Value, code: &str) -> bool {
    value["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["code"] == code)
}

#[test]
#[ignore = "requires root-owned runtime identity visible in the native Cally lane"]
fn inspection_admits_bound_forms_without_claiming_execution_or_availability() {
    required();
    let fixture = Fixture::new();
    for args in [
        vec!["git", "status"],
        vec!["git", "diff", "--cached", "--stat"],
        vec!["git", "log", "-n", "3"],
        vec!["rg", "--files"],
        vec!["rg", "-F", "-e", "λ; $(literal)", "--", "read.txt"],
    ] {
        let (output, value) = fixture.run("inspect", &args);
        assert_eq!(output.status.code(), Some(0), "{value}");
        assert_eq!(value["data"]["profile"], "linux-read-v1");
        assert_eq!(value["data"]["execution_performed"], false);
        assert_eq!(value["data"]["isolation"]["state"], "required");
        assert_eq!(value["data"]["tool"]["execution_confirmed"], false);
        assert!(value["data"]["supervisor"].is_null());
        assert_eq!(value["data"]["runtime"].as_array().unwrap().len(), 6);
    }
}

#[test]
fn malformed_policy_bindings_paths_and_wire_authority_are_denied() {
    let fixture = Fixture::new();
    for argv in [
        vec!["git", "-c", "alias.x=!touch marker", "x"],
        vec!["git", "diff", "--ext-diff"],
        vec!["git", "log", "--format=%G?"],
        vec!["rg", "--pre", "touch marker", "-e", "x"],
        vec!["rg", "-e", "x", "--", "../outside"],
        vec!["git", "status", "--short", "--porcelain=v1"],
        vec!["rg", "--files", "--line-number"],
        vec!["rg", "-n", "--line-number", "-e", "x"],
        vec!["/usr/bin/git", "status"],
    ] {
        let (output, value) = fixture.run("exec", &argv);
        assert_eq!(output.status.code(), Some(2));
        assert!(has(&value, "read_command_denied"));
        assert_eq!(value["effect_outcome"], "not_attempted");
    }
    let original = fs::read(&fixture.policy).unwrap();
    for payload in [
        b"[]".as_slice(),
        b"{\"version\":1,\"version\":1}",
        b"{\"version\":1,\"profile\":\"linux-read-v1\",\"roots\":[],\"executables\":[]}",
    ] {
        fs::write(&fixture.policy, payload).unwrap();
        let (_, value) = fixture.run("exec", &["rg", "--files"]);
        assert!(has(&value, "invalid_read_policy"));
    }
    fs::write(&fixture.policy, &original).unwrap();
    fixture
        .edit(|p| p["executables"]["rg"]["sha256"] = json!(format!("sha256:{}", "0".repeat(64))));
    let (_, value) = fixture.run("exec", &["rg", "--files"]);
    assert!(has(&value, "read_digest_mismatch"));
    fs::write(&fixture.policy, &original).unwrap();
    std::os::unix::fs::symlink(fixture.base.join("outside"), fixture.root.join("link")).unwrap();
    let (_, value) = fixture.run("exec", &["rg", "-e", "x", "--", "link"]);
    assert!(has(&value, "read_path_denied"));
    let request = fixture.base.join("request.json");
    fs::write(&request,serde_json::to_vec(&json!({"spec_version":"0.1","request_id":"fixture","operation":"process.exec","operation_version":1,"target":{"kind":"local","id":"workstation"},"inputs":{"program":"rg","args":["--files"],"cwd":fixture.root,"read_policy":"other"},"limits":{"timeout_ms":1000,"max_output_bytes":1024},"record":"never"})).unwrap()).unwrap();
    let mut call = Command::new(env!("CARGO_BIN_EXE_save"));
    call.env_clear()
        .args(["--json", "--read-policy"])
        .arg(&fixture.policy)
        .args(["call", "--request"])
        .arg(request);
    let (_, value) = run(call);
    assert!(has(&value, "invalid_process_input"));
}

#[test]
fn rg_metadata_paths_are_denied_by_cli_and_call_before_dispatch() {
    let fixture = Fixture::new();
    let request_path = fixture.base.join("metadata-request.json");
    for path in [
        ".git",
        ".git/config",
        "./.git/config",
        "././.git/config",
        ".git//config",
        "nested/.git",
        "nested/.git/config",
        "nested/./.git/config",
        "nested//.git/config",
    ] {
        for argv in [
            vec!["rg", "--files", "--", path],
            vec!["rg", "-F", "-e", "repositoryformatversion", "--", path],
        ] {
            let mut results = vec![fixture.run("exec", &argv), fixture.run("inspect", &argv)];
            fs::write(&request_path, serde_json::to_vec(&json!({"spec_version":"0.1","request_id":"metadata-refusal","operation":"process.exec","operation_version":1,"target":{"kind":"local","id":"workstation"},"inputs":{"program":"rg","args":argv[1..],"cwd":fixture.root},"limits":{"timeout_ms":3000,"max_output_bytes":1048576},"record":"never"})).unwrap()).unwrap();
            let mut call = Command::new(env!("CARGO_BIN_EXE_save"));
            call.env_clear()
                .args(["--json", "--read-policy"])
                .arg(&fixture.policy)
                .args(["call", "--request"])
                .arg(&request_path);
            results.push(run(call));
            for (output, value) in results {
                assert_eq!(output.status.code(), Some(2), "{value}");
                assert!(has(&value, "read_command_denied"), "{argv:?}: {value}");
                assert_eq!(value["effect_outcome"], "not_attempted");
                assert!(value["execution"]["child_exit_code"].is_null());
                assert_eq!(value["resolved_target"], json!({}));
                assert_eq!(value["data"], json!({}));
                assert_eq!(value["output"]["stdout"], "");
                assert_eq!(value["output"]["stderr"], "");
            }
        }
    }
}

#[test]
#[ignore = "requires the explicit native Cally containment lane"]
fn native_rg_metadata_is_omitted_while_ordinary_paths_work() {
    required();
    let fixture = Fixture::new();
    for (path, contents) in [
        ("nested/.git/config", "metadata needle\n"),
        ("linked/.git", "metadata needle\n"),
        ("nested/read.txt", "ordinary needle\n"),
        (".github/read.txt", "ordinary needle\n"),
        ("nested/file.git", "ordinary needle\n"),
    ] {
        let path = fixture.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    for paths in [
        vec!["."],
        vec!["nested", ".github", "linked"],
        vec!["nested/./read.txt", ".github/read.txt", "nested/file.git"],
    ] {
        for mut argv in [
            vec!["rg", "--files", "--hidden", "--"],
            vec!["rg", "--hidden", "-F", "-e", "needle", "--"],
        ] {
            argv.extend(paths.iter().copied());
            let (output, value) = fixture.run("exec", &argv);
            assert_eq!(output.status.code(), Some(0), "{value}");
            assert_eq!(value["data"]["tool"]["execution_confirmed"], true);
            let stdout = value["output"]["stdout"].as_str().unwrap();
            assert!(!stdout.contains("metadata needle"), "{value}");
            assert!(
                !stdout.lines().any(|line| line
                    .split('/')
                    .any(|part| part == ".git" || part.starts_with(".git:"))),
                "{value}"
            );
            for ordinary in ["read.txt", ".github/read.txt", "file.git"] {
                assert!(stdout.contains(ordinary), "{ordinary}: {value}");
            }
        }
    }
}

#[test]
fn unsupported_repository_layout_and_implicit_cwd_fail_before_wrapper() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join(".git/commondir"), "../other").unwrap();
    let (output, value) = fixture.run("exec", &["git", "status"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(has(&value, "unsupported_repository"));
    assert_eq!(value["effect_outcome"], "not_attempted");
    let mut command = Command::new(env!("CARGO_BIN_EXE_save"));
    command
        .env_clear()
        .args(["--json", "--read-policy"])
        .arg(&fixture.policy)
        .args(["exec", "--", "rg", "--files"]);
    let (_, value) = run(command);
    assert!(has(&value, "explicit_cwd_required"));
}

#[test]
fn named_unsupported_git_layouts_are_refused_before_execution() {
    for layout in [
        "bare",
        "linked",
        "sha256",
        "sparse",
        "partial",
        "partial-filter",
        "shallow",
        "alternates",
    ] {
        let mut fixture = Fixture::new();
        match layout {
            "bare" | "linked" | "sha256" => {
                let destination = fixture.base.join(layout);
                let target = destination.to_str().unwrap();
                match layout {
                    "bare" => fixture.git(&["init", "--bare", "--quiet", "--template=", target]),
                    "linked" => fixture.git(&["worktree", "add", "--detach", "--quiet", target]),
                    "sha256" => fixture.git(&[
                        "init",
                        "--object-format=sha256",
                        "--quiet",
                        "--template=",
                        target,
                    ]),
                    _ => unreachable!(),
                }
                fixture.root = destination;
                fixture.edit(|p| p["roots"][0]["path"] = json!(fixture.root));
            }
            "sparse" => fixture.git(&["sparse-checkout", "init", "--cone"]),
            "partial" => fixture.git(&["config", "remote.origin.promisor", "true"]),
            "partial-filter" => {
                fixture.git(&["config", "remote.origin.partialclonefilter", "blob:none"])
            }
            "shallow" => {
                let head = fs::read_to_string(fixture.root.join(".git/HEAD")).unwrap();
                let reference = head.trim().strip_prefix("ref: ").unwrap();
                let oid = fs::read(fixture.root.join(".git").join(reference)).unwrap();
                fs::write(fixture.root.join(".git/shallow"), oid).unwrap();
            }
            "alternates" => {
                let objects = fixture.base.join("alternate-objects");
                fs::create_dir(&objects).unwrap();
                fs::write(
                    fixture.root.join(".git/objects/info/alternates"),
                    format!("{}\n", objects.display()),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        for operation in ["exec", "inspect"] {
            let (output, value) = fixture.run(operation, &["git", "status"]);
            assert_eq!(output.status.code(), Some(2), "{layout}: {value}");
            assert!(has(&value, "unsupported_repository"), "{layout}: {value}");
            assert_eq!(value["execution"]["status"], "unsupported");
            assert_eq!(value["effect_outcome"], "not_attempted");
            assert_eq!(value["resolved_target"], json!({}));
            assert_eq!(value["output"]["stdout"], "");
            assert_eq!(value["output"]["stderr"], "");
        }
    }
}

#[test]
#[ignore = "requires the explicit native Cally containment lane"]
fn native_read_only_fifo_wait_is_bounded_by_the_operation_deadline() {
    required();
    let fixture = Fixture::new();
    let path = std::ffi::CString::new(fixture.root.join("blocked.fifo").to_str().unwrap()).unwrap();
    // SAFETY: the FIFO path is in this private fixture; no writer is ever opened.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let mut command = Command::new(env!("CARGO_BIN_EXE_save"));
    command
        .env_clear()
        .args(["--json", "--read-policy"])
        .arg(&fixture.policy)
        .args(["exec", "--cwd"])
        .arg(&fixture.root)
        .args([
            "--timeout",
            "1s",
            "--",
            "rg",
            "-F",
            "-e",
            "needle",
            "--",
            "blocked.fifo",
        ]);
    let started = Instant::now();
    let (output, value) = run(command);
    assert_eq!(output.status.code(), Some(1), "{value}");
    assert_eq!(value["execution"]["status"], "timed_out", "{value}");
    // A timeout during image hashing cannot satisfy this specific FIFO-read proof.
    assert_eq!(
        value["data"]["launcher"]["state"], "tool_started",
        "{value}"
    );
    assert_eq!(value["data"]["execution_performed"], true);
    assert_eq!(value["data"]["tool"]["execution_confirmed"], false);
    assert_eq!(value["output"]["stdout"], "");
    assert_eq!(value["output"]["stderr"], "");
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[test]
#[ignore = "requires the explicit native Cally read-only runtime fixture"]
fn native_missing_fixed_runtime_library_refuses_before_wrapper_execution() {
    required();
    let fixture = Fixture::new();
    let (_, inspected) = fixture.run("inspect", &["rg", "--files"]);
    assert_eq!(inspected["execution"]["status"], "succeeded", "{inspected}");
    // A private library-directory fixture contains only copies of declared runtime
    // bytes, with pcre2 absent. The tested program is the existing save image.
    // This changes neither the host libraries nor production binding selection.
    let view = fixture.base.join("runtime-view");
    fs::create_dir(&view).unwrap();
    fs::set_permissions(&view, fs::Permissions::from_mode(0o755)).unwrap();
    for binding in inspected["data"]["runtime"].as_array().unwrap() {
        let path = Path::new(binding["path"].as_str().unwrap());
        if path.file_name().unwrap() == "libpcre2-8.so.0" {
            continue;
        }
        let copy = view.join(path.file_name().unwrap());
        fs::copy(path, &copy).unwrap();
        fs::set_permissions(copy, fs::Permissions::from_mode(0o555)).unwrap();
    }
    let runtime_directory = fs::canonicalize("/lib/x86_64-linux-gnu").unwrap();
    let mut command = Command::new(tool("bwrap"));
    command
        .env_clear()
        .args([
            "--unshare-user",
            "--unshare-net",
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--die-with-parent",
            "--new-session",
            "--cap-drop",
            "ALL",
            "--ro-bind",
            "/",
            "/",
            "--ro-bind",
        ])
        .arg(view)
        .arg(runtime_directory)
        .args(["--", env!("CARGO_BIN_EXE_save"), "--json", "--read-policy"])
        .arg(&fixture.policy)
        .args(["exec", "--cwd"])
        .arg(&fixture.root)
        .args(["--", "rg", "--files"]);
    let (output, value) = run(command);
    assert_eq!(output.status.code(), Some(2), "{value}");
    assert!(has(&value, "unsupported_read_runtime"), "{value}");
    assert_eq!(value["execution"]["status"], "unsupported");
    assert_eq!(value["effect_outcome"], "not_attempted");
    assert_eq!(value["resolved_target"], json!({}));
    assert_eq!(value["output"]["stdout"], "");
    assert_eq!(value["output"]["stderr"], "");
}

#[test]
fn internal_launcher_cannot_be_selected_through_public_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_save"))
        .env_clear()
        .args(["__save_linux_read_v1_launch", "0", "1"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(125));
    assert!(output.stdout.is_empty());
    let fixture = Fixture::new();
    let (_, value) = fixture.run(
        "exec",
        &[
            env!("CARGO_BIN_EXE_save"),
            "__save_linux_read_v1_launch",
            "3",
            "4",
        ],
    );
    assert!(has(&value, "read_command_denied"));
    assert_eq!(value["effect_outcome"], "not_attempted");
}

#[test]
fn unavailable_local_runtime_identity_or_nested_isolation_never_falls_back() {
    if std::env::var("WORKBENCH_PROFILE_REQUIRED").as_deref() == Ok("1") {
        return;
    }
    let fixture = Fixture::new();
    let (output, value) = fixture.run("exec", &["rg", "--files"]);
    use std::os::unix::fs::MetadataExt;
    if fs::metadata("/lib/x86_64-linux-gnu/libc.so.6")
        .unwrap()
        .uid()
        == 65534
    {
        assert_eq!(output.status.code(), Some(2), "{value}");
        assert!(has(&value, "untrusted_read_binding"), "{value}");
        assert_eq!(value["effect_outcome"], "not_attempted");
        assert_eq!(value["resolved_target"], json!({}));
        return;
    }
    assert_eq!(
        output.status.code(),
        Some(1),
        "local test requires the documented nested-namespace-refusal boundary: {value}"
    );
    assert!(has(&value, "sandbox_not_established"), "{value}");
    assert_eq!(value["data"]["tool"]["execution_confirmed"], false);
    assert_eq!(value["data"]["isolation"]["state"], "unconfirmed");
    assert!(
        !value["output"]["stdout"]
            .as_str()
            .unwrap()
            .contains("read.txt")
    );
}

#[test]
#[ignore = "requires the explicit native Cally containment lane"]
fn native_git_rg_and_structured_call_preserve_real_outcomes() {
    required();
    let fixture = Fixture::new();
    for argv in [
        vec!["git", "status"],
        vec!["git", "diff", "--stat"],
        vec!["git", "log", "-n", "1"],
        vec!["rg", "--files"],
        vec!["rg", "-F", "-e", "needle", "--", "read.txt"],
    ] {
        let (output, value) = fixture.run("exec", &argv);
        assert_eq!(output.status.code(), Some(0), "{value}");
        assert_eq!(value["data"]["tool"]["exit_status"], 0);
        assert_eq!(value["data"]["isolation"]["state"], "confirmed");
    }
    let (output, value) = fixture.run("exec", &["rg", "-e", "not-found"]);
    assert_eq!(output.status.code(), Some(1), "{value}");
    assert_eq!(value["data"]["tool"]["exit_status"], 1);
    assert_eq!(value["data"]["tool"]["execution_confirmed"], true);
    assert!(!has(&value, "sandbox_not_established"));
    let request = fixture.base.join("request.json");
    fs::write(&request,serde_json::to_vec(&json!({"spec_version":"0.1","request_id":"parity","operation":"process.exec","operation_version":1,"target":{"kind":"local","id":"workstation"},"inputs":{"program":"rg","args":["-F","-e","needle","--","read.txt"],"cwd":fixture.root},"limits":{"timeout_ms":3000,"max_output_bytes":1048576},"record":"never"})).unwrap()).unwrap();
    let mut call = Command::new(env!("CARGO_BIN_EXE_save"));
    call.env_clear()
        .args(["--json", "--read-policy"])
        .arg(&fixture.policy)
        .args(["call", "--request"])
        .arg(request);
    let (output, value) = run(call);
    assert_eq!(output.status.code(), Some(0), "{value}");
    assert!(
        value["output"]["stdout"]
            .as_str()
            .unwrap()
            .contains("synthetic needle λ")
    );
    let text = Command::new(env!("CARGO_BIN_EXE_save"))
        .env_clear()
        .arg("--read-policy")
        .arg(&fixture.policy)
        .args(["exec", "--cwd"])
        .arg(&fixture.root)
        .args(["--", "rg", "-F", "-e", "needle", "--", "read.txt"])
        .output()
        .unwrap();
    assert_eq!(text.status.code(), Some(0));
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("isolation confirmed"));
    assert!(text.contains("Tool exit status: 0"));
    assert!(text.contains("synthetic needle λ"));
}

#[test]
#[ignore = "requires the explicit native Cally containment lane"]
fn native_maximum_escaped_cli_arguments_survive_private_metadata() {
    required();
    let fixture = Fixture::new();
    let path = vec!["\u{1}".repeat(204); 5].join("/");
    let mut argv = vec![
        "rg".to_owned(),
        "-F".into(),
        "-e".into(),
        "\u{1}".repeat(4096),
        "--".into(),
    ];
    argv.extend(std::iter::repeat_n(path, 32));
    // Real rg reaches 32 missing literal paths and exits 2. The old private
    // 64 KiB metadata cap refused them before tool exec despite valid CLI input.
    let args: Vec<_> = argv.iter().map(String::as_str).collect();
    let (output, value) = fixture.run("exec", &args);
    assert_eq!(output.status.code(), Some(1), "{value}");
    assert_eq!(value["data"]["tool"]["exit_status"], 2, "{value}");
    assert_eq!(value["data"]["tool"]["execution_confirmed"], true);
    assert_eq!(value["data"]["isolation"]["state"], "confirmed");
    assert!(!has(&value, "launcher_admission_failed"));
    assert!(output.stdout.len() <= 2 * 1024 * 1024);
}

#[test]
#[ignore = "requires the explicit native Cally containment lane"]
fn native_hostile_git_and_rg_controls_cannot_execute_helpers() {
    required();
    let fixture = Fixture::new();
    const RAW: &[u8] = b"helper-execution-sentinel \t\n\n";
    const FILTERED: &[u8] = b"helper-execution-sentinel\n";
    // Git dispatches this builtin by argv[0]. The helper reads stdin and writes
    // only stdout; it needs no shell, PATH lookup or writable file. The alias
    // inside the profile selects the already bound Git image, not new code.
    let control_alias = fixture.base.join("git-stripspace");
    std::os::unix::fs::symlink(fixture.base.join("git"), &control_alias).unwrap();
    std::os::unix::fs::symlink("/tool", fixture.root.join("git-stripspace")).unwrap();
    let mut control = Command::new(&control_alias)
        .env_clear()
        .current_dir(&fixture.base)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    control.stdin.take().unwrap().write_all(RAW).unwrap();
    let control = control.wait_with_output().unwrap();
    assert!(control.status.success());
    assert_eq!(control.stdout, FILTERED);
    assert!(control.stderr.is_empty());
    fs::write(
        fixture.root.join(".gitattributes"),
        "*.txt filter=hostile diff=hostile\n",
    )
    .unwrap();
    fs::OpenOptions::new().append(true).open(fixture.root.join(".git/config")).unwrap().write_all(b"\n[filter \"hostile\"]\n clean = /workspace/git-stripspace\n smudge = /workspace/git-stripspace\n[diff \"hostile\"]\n textconv = /workspace/git-stripspace\n[core]\n fsmonitor = /workspace/git-stripspace\n pager = /workspace/git-stripspace\n").unwrap();
    fs::write(fixture.root.join("read.txt"), RAW).unwrap();
    let index_before = fs::read(fixture.root.join(".git/index")).unwrap();
    for argv in [
        vec!["git", "status"],
        vec!["git", "diff"],
        vec!["git", "log", "-n", "1"],
    ] {
        let (output, value) = fixture.run("exec", &argv);
        assert_eq!(output.status.code(), Some(0), "{value}");
        assert_eq!(value["output"]["stderr"], "", "{value}");
        if argv[1] == "diff" {
            let diff = value["output"]["stdout"].as_str().unwrap();
            // Removing only /git/config masking makes the callable clean filter
            // change this line. Read-only mounts and denied writes cannot hide it.
            assert!(
                !diff.contains("+helper-execution-sentinel\n"),
                "clean-filter execution sentinel observed: {value}"
            );
            assert!(
                diff.contains("+helper-execution-sentinel \t\n"),
                "raw content was changed by a helper: {value}"
            );
        }
    }
    assert_eq!(
        fs::read(fixture.root.join(".git/index")).unwrap(),
        index_before
    );
    let marker = fixture.root.join("marker");
    let rg_config = fixture.base.join("rg-config");
    fs::write(&rg_config, "--pre=touch marker\n").unwrap();
    let mut command = fixture.command("exec", &["rg", "-e", "helper-execution-sentinel"]);
    command
        .env("RIPGREP_CONFIG_PATH", rg_config)
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.fsmonitor")
        .env("GIT_CONFIG_VALUE_0", "touch marker");
    let (output, value) = run(command);
    assert_eq!(output.status.code(), Some(0), "{value}");
    assert!(!marker.exists());
}

#[test]
#[ignore = "requires the explicit native Cally containment lane"]
fn native_socket_fifo_fd_and_filesystem_channels_are_closed() {
    required();
    let fixture = Fixture::new();
    fs::write(fixture.base.join("outside"), "outside-canary").unwrap();
    std::os::unix::fs::symlink(
        fixture.base.join("outside"),
        fixture.root.join("outside-link"),
    )
    .unwrap();
    let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let socket = UnixListener::bind(fixture.root.join("canary.sock")).unwrap();
    socket.set_nonblocking(true).unwrap();
    // Positive control: this pathname socket really reaches the outer listener.
    std::os::unix::net::UnixStream::connect(fixture.root.join("canary.sock")).unwrap();
    socket.accept().unwrap();
    let fifo = std::ffi::CString::new(fixture.root.join("canary.fifo").to_str().unwrap()).unwrap();
    // SAFETY: mkfifo receives a valid path in this private fixture directory.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let mut reader = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(fixture.root.join("canary.fifo"))
        .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(fixture.root.join("canary.fifo"))
        .unwrap()
        .write_all(b"positive")
        .unwrap();
    let mut data = [0_u8; 16];
    assert_eq!(reader.read(&mut data).unwrap(), 8);
    let outside = fs::File::open(fixture.base.join("outside")).unwrap();
    assert_eq!(
        // SAFETY: deliberately make this private canary fd inheritable for the test.
        unsafe { libc::fcntl(outside.as_raw_fd(), libc::F_SETFD, 0) },
        0
    );
    fixture.fixture_binding(json!({"mode":"boundary","port":tcp.local_addr().unwrap().port()}));
    let mut command = fixture.command("exec", &["rg", "--files"]);
    command.env("PROFILE_CANARY_SECRET", "private-fixture-only");
    let (output, value) = run(command);
    assert_eq!(output.status.code(), Some(0), "{value}");
    let report: Value = serde_json::from_str(value["output"]["stdout"].as_str().unwrap()).unwrap();
    assert_eq!(report["read"], "synthetic needle λ\n");
    for name in ["outside_denied", "proc_absent"] {
        assert_eq!(report[name], true, "{report}");
    }
    for name in ["write_errno", "tcp_errno", "unix_errno"] {
        assert_eq!(report[name], libc::EPERM, "{report}");
    }
    for name in ["fifo_errno", "fifo_readwrite_errno", "device_errno"] {
        assert_eq!(report[name], libc::EACCES, "{report}");
    }
    assert_eq!(report["legacy_fifo_errno"], libc::EACCES, "{report}");
    assert_eq!(report["readonly_truncate_errno"], libc::EPERM, "{report}");
    assert!(
        matches!(report["file_write_errno"].as_i64(), Some(code) if code == libc::EROFS as i64 || code == libc::EACCES as i64),
        "{report}"
    );
    assert_eq!(report["null_write"], true);
    assert_eq!(report["unexpected_fds"], json!([]));
    assert_eq!(report["no_new_privileges"], 1);
    assert!(report["ambient_secret"].is_null());
    assert!(!fixture.root.join("write-canary").exists());
    assert!(socket.accept().is_err());
    assert_eq!(reader.read(&mut data).unwrap(), 0);
    assert_eq!(
        fs::read_to_string(fixture.root.join("read.txt")).unwrap(),
        "synthetic needle λ\n"
    );
}

#[test]
#[ignore = "requires the explicit native Cally containment lane"]
fn native_alternative_syscall_abis_are_refused() {
    required();
    let fixture = Fixture::new();
    for mode in ["x32", "i386"] {
        fixture.fixture_binding(json!({"mode":mode}));
        let (output, value) = fixture.run("exec", &["rg", "--files"]);
        assert_eq!(output.status.code(), Some(1), "{value}");
        assert_eq!(
            value["data"]["tool"]["exit_status"],
            128 + libc::SIGSYS,
            "{value}"
        );
        assert_eq!(value["data"]["tool"]["execution_confirmed"], true);
    }
}

#[test]
#[ignore = "requires the explicit native Cally containment lane"]
fn native_failed_exec_does_not_claim_tool_execution() {
    required();
    let fixture = Fixture::new();
    let mut elf = fs::read(env!("CARGO_BIN_EXE_profile-fixture")).unwrap();
    let offset = u64::from_le_bytes(elf[32..40].try_into().unwrap()) as usize;
    let entry = u16::from_le_bytes(elf[54..56].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(elf[56..58].try_into().unwrap()) as usize;
    let mut changed = false;
    for index in 0..count {
        let header = offset + index * entry;
        if u32::from_le_bytes(elf[header..header + 4].try_into().unwrap()) == 3 {
            let start =
                u64::from_le_bytes(elf[header + 8..header + 16].try_into().unwrap()) as usize;
            let size =
                u64::from_le_bytes(elf[header + 32..header + 40].try_into().unwrap()) as usize;
            assert!(size >= 9);
            elf[start..start + size].fill(0);
            elf[start..start + 8].copy_from_slice(b"/missing");
            changed = true;
        }
    }
    assert!(changed);
    let broken = fixture.base.join("missing-loader");
    fs::write(&broken, elf).unwrap();
    fs::set_permissions(&broken, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.edit(|p| p["executables"]["rg"] = json!({"path":broken,"sha256":digest(&broken)}));
    let (output, value) = fixture.run("exec", &["rg", "--files"]);
    assert_eq!(output.status.code(), Some(1), "{value}");
    assert!(has(&value, "tool_exec_failed"), "{value}");
    assert_eq!(value["data"]["tool"]["execution_confirmed"], false);
    assert!(value["data"]["tool"]["exit_status"].is_null());
}

#[test]
#[ignore = "requires the explicit native Cally containment lane"]
fn native_unexpected_launcher_death_cannot_confirm_tool_outcome() {
    required();
    let fixture = Fixture::new();
    fixture.fixture_binding(json!({"mode":"kill-launcher"}));
    let (output, value) = run(fixture.with_limits("3s", "1024"));
    assert_eq!(output.status.code(), Some(1), "{value}");
    assert!(has(&value, "launcher_unconfirmed"), "{value}");
    assert_eq!(value["data"]["tool"]["execution_confirmed"], false);
    assert!(value["data"]["tool"]["exit_status"].is_null());
    assert_eq!(value["data"]["isolation"]["state"], "unconfirmed");
}

#[test]
#[ignore = "requires the explicit native Cally containment lane"]
fn native_output_deadline_and_cancellation_keep_bounds() {
    required();
    let fixture = Fixture::new();
    fixture.fixture_binding(json!({"mode":"flood"}));
    let (_, value) = run(fixture.with_limits("5s", "1024"));
    assert_eq!(value["output"]["stdout_truncated"], true);
    assert_eq!(value["execution"]["status"], "partial");
    fixture.fixture_binding(json!({"mode":"sleep"}));
    let began = Instant::now();
    let (_, value) = run(fixture.with_limits("200ms", "1024"));
    assert_eq!(value["execution"]["status"], "timed_out", "{value}");
    assert!(began.elapsed() < Duration::from_secs(4));
    let mut child = fixture
        .with_limits("10s", "1024")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(250));
    // SAFETY: the PID belongs to the CLI child created immediately above.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
    let began = Instant::now();
    while child.try_wait().unwrap().is_none() && began.elapsed() < Duration::from_secs(4) {
        std::thread::sleep(Duration::from_millis(20));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
        panic!("cancellation exceeded bound");
    }
    let output = child.wait_with_output().unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(143));
    assert_eq!(value["data"]["cancellation_signal"], libc::SIGTERM);
}

#[test]
#[ignore = "requires the explicit native Cally containment lane"]
fn native_detached_descendant_dies_with_namespace_parent() {
    required();
    let fixture = Fixture::new();
    let name = format!("rdchild{}", std::process::id());
    fixture.fixture_binding(json!({"mode":"detached","name":name}));
    let (output, value) = run(fixture.with_limits("3s", "1024"));
    assert_eq!(output.status.code(), Some(0), "{value}");
    std::thread::sleep(Duration::from_millis(100));
    for entry in fs::read_dir("/proc").unwrap().flatten() {
        if let Ok(comm) = fs::read_to_string(entry.path().join("comm")) {
            assert_ne!(
                comm.trim(),
                name,
                "detached fixture survived namespace teardown"
            );
        }
    }
}
