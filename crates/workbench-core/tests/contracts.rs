use serde_json::{Value, json};
use workbench_core::{
    RunControl, execute,
    request::{Limits, MAX_REQUEST_BYTES, MAX_RESULT_BYTES, Request, parse_request},
    result::{OperationResult, Status},
};

fn request_json() -> Value {
    json!({"spec_version":"0.1","request_id":"fixture-request","operation":"process.exec","operation_version":1,
        "target":{"kind":"local","id":"workstation"}, "inputs":{"program":"printf","args":["%s", "hello"],"cwd":"/tmp"},
        "limits":{"timeout_ms":30000,"max_output_bytes":1048576},"record":"never"})
}

#[test]
fn strict_controls_reject_unknown_duplicate_null_and_wrong_types() {
    for pointer in ["role", "grants", "authorization", "environment"] {
        let mut value = request_json();
        value[pointer] = json!("admin");
        let error = parse_request(value.to_string().as_bytes()).expect_err("extra control denied");
        assert_eq!(error.code, "invalid_request", "{pointer}");
    }
    for (object, field) in [
        ("target", "environment"),
        ("limits", "max_items"),
        ("limits", "concurrency"),
    ] {
        let mut value = request_json();
        value[object][field] = Value::Null;
        assert_eq!(
            parse_request(value.to_string().as_bytes())
                .unwrap_err()
                .code,
            "invalid_request"
        );
    }
    let duplicate = request_json().to_string().replacen(
        "\"record\":\"never\"",
        "\"record\":\"always\",\"record\":\"never\"",
        1,
    );
    assert_eq!(
        parse_request(duplicate.as_bytes()).unwrap_err().code,
        "invalid_json"
    );
    let duplicate = request_json().to_string().replacen(
        "\"cwd\":\"/tmp\"",
        "\"cwd\":\"/\",\"cwd\":\"/tmp\"",
        1,
    );
    assert_eq!(
        parse_request(duplicate.as_bytes()).unwrap_err().code,
        "invalid_json"
    );
    let mut value = request_json();
    value["limits"]["timeout_ms"] = json!("30000");
    assert_eq!(
        parse_request(value.to_string().as_bytes())
            .unwrap_err()
            .code,
        "invalid_request"
    );
}

#[test]
fn request_byte_depth_and_json_boundaries_are_enforced() {
    let mut exact = request_json().to_string();
    exact.extend(std::iter::repeat_n(' ', MAX_REQUEST_BYTES - exact.len()));
    assert!(parse_request(exact.as_bytes()).is_ok());
    exact.push(' ');
    assert_eq!(
        parse_request(exact.as_bytes()).unwrap_err().code,
        "request_too_large"
    );
    let deep = format!("{}0{}", "[".repeat(17), "]".repeat(17));
    assert_eq!(
        parse_request(deep.as_bytes()).unwrap_err().code,
        "request_too_deep"
    );
    let mut value = request_json();
    value["inputs"]["args"] = json!(["[[[[[[[[[[[[[[[[[[[[\"{\\"]);
    assert!(
        parse_request(value.to_string().as_bytes()).is_ok(),
        "brackets inside quoted strings do not add depth"
    );
    for malformed in ["{", "{} trailing", "{\"x\": NaN}", "{\"x\": 1e999}"] {
        assert_eq!(
            parse_request(malformed.as_bytes()).unwrap_err().code,
            "invalid_json"
        );
    }
}

#[test]
fn schema_object_boundaries_reject_serde_sequence_encodings() {
    let base = request_json();
    let sequence = json!([
        base["spec_version"],
        base["request_id"],
        base["operation"],
        base["operation_version"],
        base["target"],
        base["inputs"],
        base["limits"],
        base["record"]
    ]);
    // Serde's struct sequence syntax is valid for its generic data model, but not for
    // this JSON object's public schema. Prove that the admission guard makes the difference.
    assert!(serde_json::from_value::<Request>(sequence.clone()).is_ok());
    assert_eq!(
        parse_request(sequence.to_string().as_bytes())
            .unwrap_err()
            .code,
        "invalid_request"
    );
    for (field, value) in [
        ("target", json!(["local", "workstation"])),
        ("limits", json!([30000, 1048576])),
    ] {
        let mut request = base.clone();
        request[field] = value;
        assert!(serde_json::from_value::<Request>(request.clone()).is_ok());
        assert_eq!(
            parse_request(request.to_string().as_bytes())
                .unwrap_err()
                .code,
            "invalid_request"
        );
    }
    for operation in [
        "process.exec",
        "command.inspect",
        "task.run",
        "task.describe",
        "capability.describe",
    ] {
        let mut request = base.clone();
        request["operation"] = json!(operation);
        request["inputs"] = json!(["not-an-object"]);
        assert_eq!(
            parse_request(request.to_string().as_bytes())
                .unwrap_err()
                .code,
            "invalid_request"
        );
    }
    assert!(
        parse_request(base.to_string().as_bytes()).is_ok(),
        "literal argument arrays remain valid inside object inputs"
    );
}

#[test]
fn versions_targets_recording_and_invalid_limits_fail_before_dispatch() {
    let cases = [
        ("spec_version", json!("9.0"), "unsupported_version"),
        ("operation_version", json!(2), "unsupported_version"),
        ("record", json!("auto"), "unsupported_record_mode"),
        ("record", json!("always"), "unsupported_record_mode"),
        (
            "target",
            json!({"kind":"local","id":"different"}),
            "unsupported_target",
        ),
        (
            "limits",
            json!({"timeout_ms":0,"max_output_bytes":1048576}),
            "invalid_request",
        ),
        (
            "limits",
            json!({"timeout_ms":300001,"max_output_bytes":1048576}),
            "invalid_request",
        ),
        (
            "limits",
            json!({"timeout_ms":1,"max_output_bytes":1023}),
            "invalid_request",
        ),
        (
            "limits",
            json!({"timeout_ms":1,"max_output_bytes":2097153}),
            "invalid_request",
        ),
        (
            "limits",
            json!({"timeout_ms":1,"max_output_bytes":1024,"concurrency":1}),
            "unsupported_limits",
        ),
    ];
    for (field, replacement, code) in cases {
        let mut value = request_json();
        value[field] = replacement;
        let request = parse_request(value.to_string().as_bytes()).unwrap();
        let result = execute(request, &RunControl::default());
        assert_eq!(result.errors[0].code, code, "{field}");
        assert_eq!(result.effect_outcome, "not_attempted");
        assert_eq!(result.execution.child_exit_code, None);
        assert_eq!(result.exit_code(), 2);
    }
}

#[test]
fn unicode_and_escaping_cannot_exceed_the_final_encoded_receipt_cap() {
    for output in ["\0".repeat(1_048_576), "🦀".repeat(262_144)] {
        let mut result = OperationResult::new(&Request::new("process.exec", json!({})));
        result.output.stdout = output.clone();
        result.output.stderr = output;
        result.execution.status = Status::Succeeded;
        result.bound_output();
        let serialized = result.json_bytes();
        assert!(serialized.len() <= MAX_RESULT_BYTES, "{}", serialized.len());
        let parsed: Value = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(parsed["execution"]["status"], "partial");
        assert!(result.output.stdout_truncated || result.output.stderr_truncated);
        assert!(result.errors.iter().any(|e| e.code == "output_truncated"));
        assert!(
            !result.output.stdout.contains('\u{fffd}'),
            "JSON trimming preserves character boundaries"
        );
    }
}

#[test]
fn host_capture_limit_only_tightens_requested_limits() {
    let mut request = Request::new("doctor", json!({}));
    request.limits = Limits {
        max_output_bytes: MAX_RESULT_BYTES,
        ..Limits::default()
    };
    let result = execute(request, &RunControl::default());
    assert_eq!(result.effective_limits.max_output_bytes, 1_048_576);
    assert_eq!(result.record_mode, "never");
    assert_eq!(result.assessment, "not_assessed");
}
