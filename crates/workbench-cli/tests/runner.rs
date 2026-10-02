#![cfg(all(feature = "test-fixtures", target_os = "linux"))]

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const SAVE: &str = env!("CARGO_BIN_EXE_save");
const FIXTURE: &str = env!("CARGO_BIN_EXE_runner-fixture");

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "workbench-test-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn exec(args: &[&str]) -> Output {
    Command::new(SAVE)
        .args(["--json", "exec", "--cwd", "/tmp", "--", FIXTURE])
        .args(args)
        .output()
        .unwrap()
}
fn value(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "unexpected diagnostics: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "invalid JSON: {e}: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}
fn error(result: &Value, code: &str) -> bool {
    result["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["code"] == code)
}
fn request(marker: &Path) -> Value {
    json!({"spec_version":"0.1","request_id":"no-effects-canary","operation":"process.exec","operation_version":1,
        "target":{"kind":"local","id":"workstation"},"inputs":{"program":FIXTURE,"args":["mark",marker],"cwd":"/tmp"},
        "limits":{"timeout_ms":1000,"max_output_bytes":1048576},"record":"never"})
}
fn call(dir: &Temp, request: &[u8]) -> Output {
    let path = dir.0.join("request.json");
    fs::write(&path, request).unwrap();
    Command::new(SAVE)
        .args(["--json", "call", "--request"])
        .arg(path)
        .output()
        .unwrap()
}

#[test]
fn literal_arguments_unicode_and_metacharacters_never_get_shell_evaluation() {
    let dir = Temp::new();
    let marker = dir.0.join("should-not-exist");
    let substitution = format!("$(touch {})", marker.display());
    let expected = [
        "a b",
        "single'quote",
        "double\"quote",
        "☃ 🦀 café",
        "| ; & > < * ?",
        "",
        &substitution,
    ];
    let mut args = vec!["argv"];
    args.extend(expected);
    let output = exec(&args);
    let result = value(&output);
    assert_eq!(output.status.code(), Some(0));
    let actual: Vec<String> =
        serde_json::from_str(result["output"]["stdout"].as_str().unwrap()).unwrap();
    assert_eq!(actual, expected);
    assert!(!marker.exists());
    assert_eq!(result["effect_outcome"], "unknown");
    assert_eq!(result["assessment"], "not_assessed");
    assert_eq!(result["record_mode"], "never");
    assert_eq!(result["data"]["cleanup"]["confirmed"], true);
}

#[test]
fn environment_is_fixed_stdin_closed_and_path_not_inherited() {
    let output = Command::new(SAVE)
        .env("WORKBENCH_SECRET_CANARY", "synthetic-canary")
        .env("PATH", "/hostile")
        .env("LD_PRELOAD", "")
        .args(["--json", "exec", "--cwd", "/tmp", "--", FIXTURE, "env"])
        .output()
        .unwrap();
    let result = value(&output);
    let environment: Value =
        serde_json::from_str(result["output"]["stdout"].as_str().unwrap()).unwrap();
    assert_eq!(
        environment,
        json!({"PATH":"/usr/bin:/bin","LANG":"C.UTF-8","LC_ALL":"C.UTF-8","TZ":"UTC"})
    );
    assert_eq!(value(&exec(&["stdin"]))["output"]["stdout"], "0\n");
    let output = Command::new(SAVE)
        .env("PATH", "/hostile")
        .args(["--json", "exec", "--cwd", "/tmp", "--", "printf", "literal"])
        .output()
        .unwrap();
    assert_eq!(value(&output)["output"]["stdout"], "literal");
}

#[test]
fn both_streams_drain_after_capture_and_encoded_limits_and_disclose_loss() {
    let output = exec(&["noise", "1200000"]);
    let result = value(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(result["execution"]["child_exit_code"], 0);
    assert_eq!(result["execution"]["status"], "partial");
    assert_eq!(result["output"]["stdout_truncated"], true);
    assert_eq!(result["output"]["stderr_truncated"], true);
    assert_eq!(result["data"]["stdout_bytes_observed"], 1_200_000);
    assert_eq!(result["data"]["stderr_bytes_observed"], 1_200_000);
    assert!(error(&result, "output_truncated"));
    assert!(output.stdout.len() <= 2 * 1024 * 1024);
    let output = exec(&["controls", "400000"]);
    let result = value(&output);
    assert!(output.stdout.len() <= 2 * 1024 * 1024);
    assert_eq!(result["execution"]["status"], "partial");
    assert!(error(&result, "output_truncated"));
    assert_eq!(result["data"]["stdout_bytes_observed"], 400_000);
    assert_eq!(result["data"]["stderr_bytes_observed"], 400_000);
}

#[test]
fn invalid_utf8_and_raw_cut_codepoint_are_disclosed() {
    let result = value(&exec(&["invalid-utf8"]));
    assert_eq!(result["output"]["stdout"], "a�b");
    assert_eq!(result["output"]["encoding"], "utf-8-replaced");
    assert_eq!(result["execution"]["status"], "partial");
    assert!(error(&result, "output_encoding_replaced"));
    let output = Command::new(SAVE)
        .args([
            "--json",
            "exec",
            "--max-output-bytes",
            "1025",
            "--",
            FIXTURE,
            "unicode-boundary",
        ])
        .output()
        .unwrap();
    let result = value(&output);
    assert!(error(&result, "output_encoding_replaced"));
    assert!(error(&result, "output_truncated"));
    assert_eq!(result["output"]["encoding"], "utf-8-replaced");
}

#[test]
fn nonzero_primary_exit_is_preserved_separately_from_cli_status() {
    let output = exec(&["exit", "42"]);
    let result = value(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(result["execution"]["child_exit_code"], 42);
    assert_eq!(result["execution"]["status"], "failed");
    assert!(error(&result, "child_failed"));
    assert_eq!(result["effect_outcome"], "unknown");
}

#[test]
fn timeout_and_forced_termination_have_bounded_cleanup() {
    for mode in ["sleep", "ignore-term", "tree"] {
        let start = Instant::now();
        let output = Command::new(SAVE)
            .args(["--json", "exec", "--timeout", "200ms", "--", FIXTURE, mode])
            .output()
            .unwrap();
        let result = value(&output);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(result["execution"]["status"], "timed_out", "{result}");
        assert!(error(&result, "timed_out"));
        assert!(start.elapsed() < Duration::from_secs(4));
        assert_eq!(result["data"]["cleanup"]["primary_reaped"], true);
        if mode == "ignore-term" {
            assert_eq!(result["data"]["cleanup"]["kill_sent"], true);
            assert_eq!(result["execution"]["signal"], "SIGKILL");
        }
        if !result["data"]["cleanup"]["confirmed"].as_bool().unwrap() {
            assert!(error(&result, "cleanup_unconfirmed"));
        }
        assert_descendant_stopped(&result);
    }
}

#[test]
fn primary_exit_before_descendant_pipe_eof_is_not_false_completion() {
    for (mode, code) in [("descendant", 7), ("descendant-zero", 0)] {
        let start = Instant::now();
        let output = exec(&[mode]);
        let result = value(&output);
        assert!(start.elapsed() < Duration::from_secs(4));
        assert_eq!(result["execution"]["child_exit_code"], code);
        assert_eq!(
            result["data"]["cleanup"]["trigger"],
            "descendants_after_leader_exit"
        );
        assert_eq!(result["data"]["cleanup"]["term_sent"], true);
        assert_ne!(result["execution"]["status"], "succeeded");
        if code == 0 {
            assert_eq!(result["execution"]["status"], "partial");
            assert!(error(&result, "descendants_terminated"));
        }
        assert_descendant_stopped(&result);
    }
}

fn assert_descendant_stopped(result: &Value) {
    let stdout = result["output"]["stdout"].as_str().unwrap();
    if let Some(pid) = stdout.trim().strip_prefix("descendant_pid=")
        && let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat"))
    {
        let state = stat
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .next()
            .unwrap();
        assert!(
            matches!(state, "Z" | "X"),
            "descendant still running: {stat}"
        );
    }
}

#[test]
fn malformed_version_record_grant_and_path_requests_never_create_markers() {
    let dir = Temp::new();
    let marker = dir.0.join("marker");
    let base = request(&marker);
    let cases = [
        ("spec_version", json!("2"), "unsupported_version"),
        ("operation_version", json!(9), "unsupported_version"),
        ("record", json!("always"), "unsupported_record_mode"),
        ("role", json!("admin"), "invalid_request"),
        ("grant", json!("all"), "invalid_request"),
        (
            "limits",
            json!({"timeout_ms":0,"max_output_bytes":1024}),
            "invalid_request",
        ),
    ];
    for (field, replacement, expected) in cases {
        let mut input = base.clone();
        input[field] = replacement;
        let output = call(&dir, input.to_string().as_bytes());
        let result = value(&output);
        assert_eq!(output.status.code(), Some(2));
        assert!(error(&result, expected), "{result}");
        assert_eq!(result["effect_outcome"], "not_attempted");
        assert!(!marker.exists());
    }
    for (field, replacement, expected) in [
        ("cwd", "relative", "absolute_cwd_required"),
        ("cwd", "/does-not-exist-workbench", "invalid_cwd"),
        ("program", "./fixture", "absolute_program_required"),
        (
            "program",
            "workbench-does-not-exist",
            "executable_not_found",
        ),
        ("program", "/bin/sh", "shell_not_supported"),
    ] {
        let mut input = base.clone();
        input["inputs"][field] = json!(replacement);
        let result = value(&call(&dir, input.to_string().as_bytes()));
        assert!(error(&result, expected), "{result}");
        assert_eq!(result["effect_outcome"], "not_attempted");
        assert!(!marker.exists());
    }
    assert!(error(&value(&call(&dir, b"{")), "invalid_json"));
    assert!(!marker.exists());
}

#[test]
fn inspect_canonicalizes_without_running_and_discovery_is_offline() {
    let dir = Temp::new();
    let marker = dir.0.join("marker");
    let output = Command::new(SAVE)
        .args([
            "--json",
            "command",
            "inspect",
            "--cwd",
            dir.path(),
            "--",
            FIXTURE,
            "mark",
        ])
        .arg(&marker)
        .output()
        .unwrap();
    let result = value(&output);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(result["data"]["execution_performed"], false);
    assert_eq!(result["resolved_target"]["cwd"], dir.path());
    assert_eq!(result["execution"]["child_exit_code"], Value::Null);
    assert!(!marker.exists());
    for args in [
        vec!["--json", "doctor"],
        vec!["--json", "capabilities", "list"],
        vec!["--json", "capabilities", "describe", "process.exec"],
    ] {
        let output = Command::new(SAVE).args(args).output().unwrap();
        let result = value(&output);
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(result["effect_outcome"], "not_applicable");
        assert_eq!(result["execution"]["child_exit_code"], Value::Null);
    }
}

#[test]
fn text_and_json_report_the_same_execution_and_literal_stdout() {
    let json = value(&exec(&["argv", "hello"]));
    let output = Command::new(SAVE)
        .args(["exec", "--cwd", "/tmp", "--", FIXTURE, "argv", "hello"])
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(text.starts_with("process.exec: succeeded\n"));
    assert!(text.contains("Child exit: 0\n"));
    assert!(text.contains(json["output"]["stdout"].as_str().unwrap()));
    assert!(text.contains("Assessment: not_assessed\n"));
}

fn wait_bounded(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("runner failed to exit within 4 seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn closed_or_stalled_result_consumers_exit_without_panicking_or_hanging() {
    let mut child = Command::new(SAVE)
        .args(["--json", "exec", "--", FIXTURE, "sleep"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    assert_eq!(wait_bounded(&mut child).code(), Some(1));
    let output = child.wait_with_output().unwrap();
    assert!(output.stderr.is_empty());
    let mut child = Command::new(SAVE)
        .args(["--json", "exec", "--", FIXTURE, "noise", "1000000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(wait_bounded(&mut child).code(), Some(1));
    let output = child.wait_with_output().unwrap();
    assert!(output.stderr.is_empty());
}

#[test]
fn sigint_and_sigterm_cancel_owned_children_with_normalized_exit() {
    for (signal, code) in [(libc::SIGINT, 130), (libc::SIGTERM, 143)] {
        let dir = Temp::new();
        let ready = dir.0.join("ready");
        let mut child = Command::new(SAVE)
            .args(["--json", "exec", "--", FIXTURE, "ready-file"])
            .arg(&ready)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !ready.exists() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("fixture did not signal readiness");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        // SAFETY: child.id() is the live runner created by this test; signal is SIGINT/SIGTERM.
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        assert_eq!(wait_bounded(&mut child).code(), Some(code));
        let output = child.wait_with_output().unwrap();
        let result = value(&output);
        assert_eq!(result["execution"]["status"], "cancelled");
        assert_eq!(result["data"]["cancellation_signal"], signal);
        assert_eq!(result["data"]["cleanup"]["primary_reaped"], true);
        assert_eq!(result["data"]["cleanup"]["confirmed"], true);
    }
}

#[test]
fn late_signal_during_timeout_cleanup_preserves_timeout_and_signal_exit() {
    for (signal, code) in [(libc::SIGINT, 130), (libc::SIGTERM, 143)] {
        let dir = Temp::new();
        let term_marker = dir.0.join("term-received");
        let mut child = Command::new(SAVE)
            .args([
                "--json",
                "exec",
                "--timeout",
                "200ms",
                "--",
                FIXTURE,
                "ignore-term",
            ])
            .arg(&term_marker)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !term_marker.exists() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("fixture never observed cleanup SIGTERM");
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        // The fixture marker proves the supervisor already selected timeout cleanup.
        // SAFETY: signal targets the live runner created by this test.
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        let output = child.wait_with_output().unwrap();
        let result = value(&output);
        assert_eq!(result["execution"]["status"], "timed_out");
        assert_eq!(result["data"]["cleanup"]["trigger"], "timeout");
        assert_eq!(result["execution"]["signal"], "SIGKILL");
        assert_eq!(result["effect_outcome"], "unknown");
        assert_eq!(
            result["data"]["cancellation_signal"], signal,
            "late signal must be retained in the receipt"
        );
        assert_eq!(output.status.code(), Some(code));
    }
}

#[test]
fn late_signal_interrupts_stalled_result_delivery_with_signal_exit() {
    use std::os::fd::AsRawFd;
    for (signal, code) in [(libc::SIGINT, 130), (libc::SIGTERM, 143)] {
        let mut child = Command::new(SAVE)
            .args(["--json", "exec", "--", FIXTURE, "noise", "1000000"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut poll = libc::pollfd {
            fd: child.stdout.as_ref().unwrap().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // Single-result output starts only after process supervision finishes. Wait for
        // its first bytes without draining: the pipe then blocks the large receipt.
        // SAFETY: poll references one initialized element owning a live pipe fd.
        let ready = unsafe { libc::poll(&mut poll, 1, 2000) };
        assert!(
            ready > 0 && poll.revents & libc::POLLIN != 0,
            "receipt delivery did not start"
        );
        let sent_at = Instant::now();
        // SAFETY: signal targets the live runner created by this test.
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        let status = wait_bounded(&mut child);
        assert_eq!(status.code(), Some(code));
        assert!(
            sent_at.elapsed() < Duration::from_millis(250),
            "delivery ignored cancellation until its independent timeout"
        );
        let output = child.wait_with_output().unwrap();
        assert!(output.stderr.is_empty());
    }
}
