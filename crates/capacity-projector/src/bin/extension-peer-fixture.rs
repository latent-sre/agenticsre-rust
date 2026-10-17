//! Deliberately nonconforming peers for isolated host protocol verification.
//! This binary is absent unless the explicit test-fixtures feature is enabled.
use capacity_projector::{
    Hello, Request, read_frame, validate_hello, validate_request, write_frame,
};
use serde_json::{Value, json};
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Duration;

fn frame(hello: &Hello, sequence: u64, kind: &str) -> Value {
    json!({"protocol":1,"request_id":hello.request_id,"run_id":hello.run_id,"sequence":sequence,"kind":kind})
}

fn terminal(hello: &Hello, request: &Request) -> Value {
    let mut value = frame(hello, 2, "result");
    value["status"] = json!("succeeded");
    value["data"] =
        serde_json::to_value(capacity_projector::project(&request.inputs).unwrap()).unwrap();
    value["error"] = Value::Null;
    value
}

fn run() -> io::Result<u8> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let hello: Hello = read_frame(&mut input)?;
    validate_hello(&hello)?;
    // A closed set avoids turning the test-only marker into a path interface.
    let mode = match hello.request_id.as_str() {
        "fixture-unsupported" => "unsupported",
        "fixture-correlation" => "correlation",
        "fixture-malformed" => "malformed",
        "fixture-invalid-utf8" => "invalid-utf8",
        "fixture-oversized" => "oversized",
        "fixture-sequence" => "sequence",
        "fixture-early-result" => "early-result",
        "fixture-duplicate-terminal" => "duplicate-terminal",
        "fixture-trailing-progress" => "trailing-progress",
        "fixture-missing-terminal" => "missing-terminal",
        "fixture-failed" => "failed",
        "fixture-nonzero" => "nonzero",
        "fixture-timeout" => "timeout",
        "fixture-stubborn" => "stubborn",
        "fixture-crash-beforehello" => "crash-beforehello",
        "fixture-normal" => "normal",
        "fixture-stderr-flood" => "stderr-flood",
        "fixture-progress-flood" => "progress-flood",
        "fixture-invalid-output" => "invalid-output",
        "fixture-unterminated" => "unterminated",
        "fixture-duplicate-key" => "duplicate-key",
        "fixture-array-frame" => "array-frame",
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown fixture request_id",
            ));
        }
    };
    match mode {
        "unsupported" => {
            let mut response = frame(&hello, 0, "hello");
            response["protocol"] = json!(2);
            write_frame(&mut output, &response)?;
            return Ok(0);
        }
        "correlation" => {
            let mut response = frame(&hello, 0, "hello");
            response["run_id"] = json!("uncorrelated-fixture-run");
            write_frame(&mut output, &response)?;
            return Ok(0);
        }
        "malformed" => {
            output.write_all(b"{this is not JSON}\n")?;
            output.flush()?;
            return Ok(0);
        }
        "invalid-utf8" => {
            output.write_all(&[0xff, b'\n'])?;
            output.flush()?;
            return Ok(0);
        }
        "oversized" => {
            output.write_all(&vec![b'x'; capacity_projector::MAX_FRAME_BYTES + 1])?;
            output.flush()?;
            return Ok(0);
        }
        "early-result" => {
            let mut response = frame(&hello, 0, "result");
            response["status"] = json!("succeeded");
            response["data"] = json!({});
            response["error"] = Value::Null;
            write_frame(&mut output, &response)?;
            return Ok(0);
        }
        "unterminated" => {
            output.write_all(b"{\"protocol\":1")?;
            output.flush()?;
            return Ok(0);
        }
        "duplicate-key" => {
            let response = serde_json::to_string(&frame(&hello, 0, "hello")).unwrap();
            output.write_all(response.replacen('{', "{\"protocol\":1,", 1).as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
            return Ok(0);
        }
        "array-frame" => {
            write_frame(
                &mut output,
                &json!([1, hello.request_id, hello.run_id, 0, "hello"]),
            )?;
            return Ok(0);
        }
        "crash-beforehello" => return Ok(7),
        _ => {}
    }
    write_frame(&mut output, &hello)?;
    let request: Request = read_frame(&mut input)?;
    validate_request(&request, &hello)?;
    if mode == "timeout" || mode == "stubborn" {
        #[cfg(unix)]
        if mode == "stubborn" {
            // SAFETY: SIG_IGN is the platform-defined disposition; no Rust
            // callback or shared state is accessed by the signal handler.
            unsafe { libc::signal(libc::SIGTERM, libc::SIG_IGN) };
        }
        std::fs::write(
            format!("/tmp/extension-peer-{}.pid", hello.request_id),
            std::process::id().to_string(),
        )?;
        std::thread::sleep(Duration::from_secs(60));
        return Ok(0);
    }
    if mode == "missing-terminal" {
        return Ok(0);
    }
    if mode == "progress-flood" {
        for sequence in 1..=65 {
            let mut progress = frame(&hello, sequence, "progress");
            progress["message"] = json!("Fixture progress flood.");
            write_frame(&mut output, &progress)?;
        }
        return Ok(0);
    }
    if mode == "stderr-flood" {
        let mut diagnostics = io::stderr().lock();
        diagnostics.write_all(&vec![b'x'; 80 * 1024])?;
        diagnostics.flush()?;
    }
    let mut progress = frame(&hello, if mode == "sequence" { 2 } else { 1 }, "progress");
    progress["message"] = json!("Fixture progress.");
    write_frame(&mut output, &progress)?;
    if mode == "sequence" {
        return Ok(0);
    }
    let mut result = terminal(&hello, &request);
    if mode == "failed" {
        result["status"] = json!("failed");
        result["data"] = Value::Null;
        result["error"] = json!({"code":"fixture_failure","message":"Explicit peer failure for protocol verification."});
    }
    if mode == "invalid-output" {
        result["data"]["headroom_bytes"] = json!("not an integer");
    }
    write_frame(&mut output, &result)?;
    match mode {
        "duplicate-terminal" => {
            result["sequence"] = json!(3);
            write_frame(&mut output, &result)?;
        }
        "trailing-progress" => {
            progress["sequence"] = json!(3);
            write_frame(&mut output, &progress)?;
        }
        "nonzero" => return Ok(7),
        _ => {}
    }
    Ok(0)
}

fn main() -> ExitCode {
    if std::env::args_os().len() != 1 {
        return ExitCode::FAILURE;
    }
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("extension-peer-fixture: {error}");
            ExitCode::FAILURE
        }
    }
}
