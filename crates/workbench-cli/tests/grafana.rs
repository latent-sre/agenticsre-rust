#![cfg(all(feature = "test-fixtures", target_os = "linux"))]
#[path = "support/grafana_https.rs"]
mod https;
use base64::{Engine, engine::general_purpose::STANDARD};
use https::*;
use serde_json::{Value, json};
use std::{
    fs,
    process::Stdio,
    time::{Duration, Instant},
};

#[test]
fn real_https_dashboard_and_both_query_dialects_use_verified_scoped_exchanges() {
    let fixture = Fixture::new(vec![
        Reply::org(),
        Reply::dashboard(),
        Reply::org(),
        Reply::datasource("prometheus"),
        Reply::query(),
        Reply::org(),
        Reply::datasource("loki"),
        Reply::query(),
    ]);
    let (output, result) = fixture.run(&mut fixture.dashboard());
    assert_eq!(output.status.code(), Some(0), "{result}");
    assert_eq!(result["data"]["response"]["dashboard"]["uid"], "board-1");
    assert_eq!(result["data"]["binding"]["organization_verified"], true);
    for kind in ["prometheus", "loki"] {
        let (output, result) = fixture.run(&mut fixture.query(kind));
        assert_eq!(output.status.code(), Some(0), "{result}");
        assert_eq!(result["coverage"]["state"], "not_established");
        assert_eq!(result["execution"]["child_exit_code"], Value::Null);
        assert_eq!(result["assessment"], "not_assessed");
        assert_eq!(result["effect_outcome"], "not_applicable");
    }
    let requests = fixture.requests();
    assert_eq!(requests.len(), 8);
    assert_eq!(requests[0].path, "/grafana/api/org");
    assert_eq!(requests[1].path, "/grafana/api/dashboards/uid/board-1");
    assert_eq!(
        requests[3].path,
        "/grafana/api/datasources/uid/metrics-1?ds_type=prometheus"
    );
    assert_eq!(
        requests[6].path,
        "/grafana/api/datasources/uid/metrics-1?ds_type=loki"
    );
    for request in &requests {
        assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
        assert_eq!(request.headers["x-grafana-org-id"], "7");
    }
    for (index, kind) in [(4, "prometheus"), (7, "loki")] {
        assert_eq!(requests[index].method, "POST");
        assert_eq!(requests[index].path, "/grafana/api/ds/query");
        let body: Value = serde_json::from_slice(&requests[index].body).unwrap();
        assert_eq!(body["from"], "1790899200000");
        assert_eq!(body["to"], "1790899260000");
        assert_eq!(body["queries"].as_array().unwrap().len(), 1);
        assert_eq!(
            body["queries"][0]["datasource"],
            json!({"uid":"metrics-1","type":kind})
        );
        assert_eq!(body["queries"][0]["expr"], "up{job=~\"café$\"}");
        assert_eq!(body["queries"][0]["maxDataPoints"], 1000);
        assert_eq!(body["queries"][0]["intervalMs"], 1000);
        if kind == "loki" {
            assert_eq!(body["queries"][0]["maxLines"], 500);
            assert_eq!(body["queries"][0]["queryType"], "range");
        } else {
            assert_eq!(body["queries"][0]["range"], true);
            assert_eq!(body["queries"][0]["instant"], false);
        }
    }
}

#[test]
fn wrong_org_datasource_and_http200_semantic_errors_stop_the_sequence() {
    for (replies, code, count) in [
        (
            vec![Reply::json(json!({"id":true}))],
            "organization_mismatch",
            1,
        ),
        (
            vec![Reply::json(json!({"id":8}))],
            "organization_mismatch",
            1,
        ),
        (
            vec![
                Reply::org(),
                Reply::json(json!({"uid":"metrics-1","type":"prometheus","orgId":8})),
            ],
            "organization_mismatch",
            2,
        ),
        (
            vec![
                Reply::org(),
                Reply::json(json!({"uid":"other","type":"prometheus"})),
            ],
            "datasource_mismatch",
            2,
        ),
        (
            vec![Reply::org(), Reply::datasource("loki")],
            "datasource_mismatch",
            2,
        ),
        (
            vec![
                Reply::org(),
                Reply::datasource("prometheus"),
                Reply::json(json!({"results":{"A":{"error":TOKEN}}})),
            ],
            "query_failed",
            3,
        ),
        (
            vec![
                Reply::org(),
                Reply::datasource("prometheus"),
                Reply::json(json!({"results":{"A":{"status":"403","frames":[]}}})),
            ],
            "query_failed",
            3,
        ),
        (
            vec![
                Reply::org(),
                Reply::datasource("prometheus"),
                Reply::json(json!({"results":{"A":{},"B":{"frames":[]}}})),
            ],
            "query_failed",
            3,
        ),
    ] {
        let fixture = Fixture::new(replies);
        let (output, result) = fixture.run(&mut fixture.query("prometheus"));
        assert_eq!(output.status.code(), Some(1), "{result}");
        assert!(has_error(&result, code), "{result}");
        assert_eq!(fixture.requests().len(), count);
        assert_eq!(result["data"]["response"], Value::Null);
        assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
    }
}

#[test]
fn private_data_is_masked_without_corrupting_public_identity_or_control_types() {
    for token in [TOKEN, "true", "false", "null", "7"] {
        let echo = serde_json::from_str::<Value>(token).unwrap_or(json!(token));
        let mut fixture = Fixture::new(vec![
            Reply::org(),
            Reply::json(
                json!({"dashboard":{"uid":"board-1","echo":echo,"nested":[format!("prefix-{token}-suffix"),STANDARD.encode(token)],"secret_key":{token:"value"},"link":"https://unknown:private@host/path","Authorization":"other-sensitive"},"meta":{"echo":echo}}),
            ),
        ]);
        fixture.provider["auth"]["token"] = json!(token);
        let (output, result) = fixture.run(&mut fixture.dashboard());
        assert_eq!(output.status.code(), Some(0), "{result}");
        assert_eq!(result["execution"]["status"], "succeeded");
        assert_eq!(result["target"]["id"], "fixture");
        assert!(result["data"]["binding"]["organization_verified"].is_boolean());
        let dashboard = &result["data"]["response"]["dashboard"];
        assert!(dashboard["echo"] == "[REDACTED]" || dashboard["echo"] == json!({"redacted":true}));
        assert_eq!(dashboard["secret_key"]["[REDACTED]"], "value");
        assert_eq!(dashboard["nested"][1], "[REDACTED]");
        assert_eq!(dashboard["link"], "https://[REDACTED]@host/path");
        assert_eq!(dashboard["Authorization"], json!({"redacted":true}));
        if token == "7" {
            assert_eq!(
                result["data"]["binding"]["organization_id"],
                json!({"redacted":true})
            );
        }
    }
    let mut fixture = Fixture::new(vec![
        Reply::org(),
        Reply::json(json!({"dashboard":{"uid":"board-1","741829":"a","928413":"b"},"meta":{}})),
    ]);
    fixture.provider["auth"] = json!({"kind":"basic","username":"741829","password":"928413"});
    let (output, result) = fixture.run(&mut fixture.dashboard());
    assert_eq!(output.status.code(), Some(1));
    assert!(has_error(&result, "redaction_key_collision"));
    assert_eq!(result["data"]["response"], Value::Null);
}

#[test]
fn tls_proxy_redirect_retry_and_ambient_trust_boundaries_are_real() {
    let wrong_name = Fixture::with_hostname(vec![Reply::org()], "wrong.invalid");
    let (output, result) = wrong_name.run(&mut wrong_name.dashboard());
    assert_eq!(output.status.code(), Some(1));
    assert!(has_error(&result, "request_failed"));
    assert!(wrong_name.requests().is_empty());
    let unknown_ca = Fixture::new(vec![Reply::org()]);
    unknown_ca.edit_config(|c| {
        c["connections"]["fixture"]
            .as_object_mut()
            .unwrap()
            .remove("tls_ca_file");
    });
    let (output, result) = unknown_ca.run(
        unknown_ca
            .dashboard()
            .env("SSL_CERT_FILE", &unknown_ca.ca)
            .env("SSL_CERT_DIR", &unknown_ca.directory),
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(has_error(&result, "request_failed"));
    assert!(unknown_ca.requests().is_empty());
    let fixture = Fixture::new(vec![Reply::org(), Reply::dashboard()]);
    let keylog = fixture.directory.join("tls-keys");
    let (output, result) = fixture.run(
        fixture
            .dashboard()
            .env("HTTPS_PROXY", "http://127.0.0.1:1")
            .env("ALL_PROXY", "http://127.0.0.1:1")
            .env("SSLKEYLOGFILE", &keylog),
    );
    assert_eq!(output.status.code(), Some(0), "{result}");
    assert!(!keylog.exists());
    for status in [302, 401, 503] {
        let mut reply = Reply::raw(TOKEN.as_bytes().to_vec());
        reply.status = status;
        reply
            .headers
            .push(("Location".into(), "/grafana/unexpected".into()));
        let fixture = Fixture::new(vec![reply, Reply::org()]);
        let (output, result) = fixture.run(&mut fixture.dashboard());
        assert_eq!(output.status.code(), Some(1));
        assert!(has_error(
            &result,
            if status == 302 {
                "redirect_rejected"
            } else {
                "http_error"
            }
        ));
        assert_eq!(fixture.requests().len(), 1);
        assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
    }
}

#[test]
fn bounded_response_parsing_rejects_duplicate_nonfinite_deep_and_excess_bytes() {
    let mut deep = "[".repeat(65);
    deep.push_str(&"]".repeat(65));
    for raw in [
        br#"{"id":7,"id":8}"#.to_vec(),
        br#"{"id":NaN}"#.to_vec(),
        br#"{"id":1e999}"#.to_vec(),
        deep.into_bytes(),
        b"not json".to_vec(),
    ] {
        let fixture = Fixture::new(vec![Reply::raw(raw)]);
        let (output, result) = fixture.run(&mut fixture.dashboard());
        assert_eq!(output.status.code(), Some(1));
        assert!(has_error(&result, "invalid_response"), "{result}");
        assert_eq!(fixture.requests().len(), 1);
    }
    for chunked in [false, true] {
        let mut reply = Reply::raw(vec![b'x'; 1025]);
        reply.chunked = chunked;
        let fixture = Fixture::new(vec![reply]);
        let (output, result) =
            fixture.run(fixture.dashboard().args(["--max-output-bytes", "1024"]));
        assert_eq!(output.status.code(), Some(1));
        assert!(has_error(&result, "response_too_large"));
    }
}

#[test]
fn invalid_admission_and_remote_looking_local_operations_never_make_http_or_child_effects() {
    let fixture = Fixture::new(vec![]);
    for patch in [
        json!({"expected_org_id":8}),
        json!({"origin":"https://127.0.0.1:1"}),
        json!({"operations":["grafana.query"]}),
    ] {
        let mut provider = fixture.provider.clone();
        for (k, v) in patch.as_object().unwrap() {
            provider[k] = v.clone();
        }
        let (output, result) = fixture.run(
            fixture
                .dashboard()
                .env("SAVE_GRAFANA_FIXTURE", provider.to_string()),
        );
        assert_eq!(output.status.code(), Some(2));
        assert!(has_error(&result, "credential_scope_mismatch"));
        assert_eq!(result["data"]["transport"]["requests_started"], 0);
    }
    for operation in [
        "process.exec",
        "command.inspect",
        "task.run",
        "task.describe",
    ] {
        let marker = fixture.directory.join("must-not-exist");
        let request = json!({"spec_version":"0.1","request_id":"target-control","operation":operation,"operation_version":1,"target":{"kind":"connection","id":"fixture"},"inputs":{"program":"/usr/bin/touch","args":[marker],"cwd":"/tmp"},"limits":{"timeout_ms":1000,"max_output_bytes":1024},"record":"never"});
        let path = fixture.directory.join("request.json");
        fs::write(&path, request.to_string()).unwrap();
        let (output, result) =
            fixture.run(fixture.command().args(["call", "--request"]).arg(&path));
        assert_eq!(output.status.code(), Some(2));
        assert!(has_error(&result, "unsupported_target"));
        assert!(!marker.exists());
    }
    assert!(fixture.requests().is_empty());
}

#[test]
fn cli_call_and_human_output_share_one_observation() {
    let fixture = Fixture::new(vec![
        Reply::org(),
        Reply::dashboard(),
        Reply::org(),
        Reply::dashboard(),
        Reply::org(),
        Reply::dashboard(),
    ]);
    let (_, cli) = fixture.run(&mut fixture.dashboard());
    let request = json!({"spec_version":"0.1","request_id":"fixture","operation":"grafana.dashboard.get","operation_version":1,"target":{"kind":"connection","id":"fixture"},"inputs":{"uid":"board-1"},"limits":{"timeout_ms":60000,"max_output_bytes":2097152},"record":"never"});
    let path = fixture.directory.join("request.json");
    fs::write(&path, request.to_string()).unwrap();
    let (output, call) = fixture.run(fixture.command().args(["call", "--request"]).arg(path));
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(call["request_id"], "fixture");
    assert_eq!(call["data"], cli["data"]);
    let output = std::process::Command::new(SAVE)
        .env_clear()
        .env("SAVE_GRAFANA_FIXTURE", fixture.provider.to_string())
        .arg("--config")
        .arg(&fixture.config)
        .args([
            "grafana",
            "dashboard",
            "get",
            "--target",
            "fixture",
            "--uid",
            "board-1",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("board-1"));
    assert!(text.contains("Fixture"));
    assert!(text.contains("Coverage:"));
    assert!(!text.contains("upstream_digest"));
}

#[test]
fn whole_operation_deadline_includes_sequential_requests_and_body_consumption() {
    let mut org = Reply::org();
    org.header_delay = Duration::from_millis(90);
    let mut dashboard = Reply::dashboard();
    dashboard.body_delay = Duration::from_millis(200);
    let fixture = Fixture::new(vec![org, dashboard]);
    let start = Instant::now();
    let (output, result) = fixture.run(fixture.dashboard().args(["--timeout", "180ms"]));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(result["execution"]["status"], "timed_out");
    assert!(has_error(&result, "deadline_exceeded"));
    assert_eq!(result["data"]["response"], Value::Null);
    assert_eq!(fixture.requests().len(), 2);
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn cancellation_closes_native_requests_with_signal_exit_and_no_later_dispatch() {
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let mut org = Reply::org();
        org.body_delay = Duration::from_secs(2);
        let fixture = Fixture::new(vec![org, Reply::dashboard()]);
        let child = fixture
            .dashboard()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        fixture.wait_for_requests(1);
        let start = Instant::now();
        // SAFETY: the PID belongs to the owned, still-running CLI child; only its cancellation signal is sent.
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        let output = child.wait_with_output().unwrap();
        let result = receipt(&output);
        assert_eq!(output.status.code(), Some(128 + signal));
        assert_eq!(result["execution"]["status"], "cancelled");
        assert_eq!(result["data"]["cancellation_signal"], signal);
        assert_eq!(result["data"]["response"], Value::Null);
        assert_eq!(fixture.requests().len(), 1);
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}

#[test]
fn secret_collisions_with_public_identifiers_preserve_correlated_typed_wrappers() {
    for token in ["fixture", "dashboard", "meta", "results", "A", "succeeded"] {
        let mut fixture = Fixture::new(vec![
            Reply::org(),
            Reply::json(json!({"dashboard":{"uid":"board-1","echo":token},"meta":{"echo":token}})),
            Reply::org(),
            Reply::datasource("prometheus"),
            Reply::json(json!({"results":{"A":{"frames":[{"echo":token}]}}})),
        ]);
        fixture.provider["auth"]["token"] = json!(token);
        let (output, dashboard) = fixture.run(&mut fixture.dashboard());
        assert_eq!(output.status.code(), Some(0), "{dashboard}");
        assert_eq!(dashboard["target"]["id"], "fixture");
        assert_eq!(dashboard["execution"]["status"], "succeeded");
        assert_eq!(
            dashboard["data"]["response"]["dashboard"]["echo"],
            "[REDACTED]"
        );
        assert_eq!(dashboard["data"]["response"]["meta"]["echo"], "[REDACTED]");
        let (output, query) = fixture.run(&mut fixture.query("prometheus"));
        assert_eq!(output.status.code(), Some(0), "{query}");
        assert_eq!(
            query["data"]["response"]["results"]["A"]["frames"][0]["echo"],
            "[REDACTED]"
        );
    }
}

#[test]
fn basic_authentication_is_exact_and_its_numeric_and_encoded_echoes_are_masked() {
    let pair = "741829:928413";
    let mut fixture = Fixture::new(vec![
        Reply::org(),
        Reply::json(
            json!({"dashboard":{"uid":"board-1","user":741829,"password":928413.0,"pair":pair,"encoded":STANDARD.encode(pair)},"meta":{}}),
        ),
    ]);
    fixture.provider["auth"] = json!({"kind":"basic","username":"741829","password":"928413"});
    let (output, result) = fixture.run(&mut fixture.dashboard());
    assert_eq!(output.status.code(), Some(0), "{result}");
    assert!(
        fixture
            .requests()
            .iter()
            .all(|r| r.headers["authorization"] == format!("Basic {}", STANDARD.encode(pair)))
    );
    let payload = &result["data"]["response"]["dashboard"];
    assert_eq!(payload["user"], json!({"redacted":true}));
    assert_eq!(payload["password"], json!({"redacted":true}));
    assert_eq!(payload["pair"], "[REDACTED]");
    assert_eq!(payload["encoded"], "[REDACTED]");
}

#[test]
fn redaction_expansion_and_envelope_overhead_cannot_escape_the_encoded_result_cap() {
    for expansion in [false, true] {
        let value = if expansion {
            "e".repeat(400_000)
        } else {
            "z".repeat(2_097_152 - 512)
        };
        let mut fixture = Fixture::new(vec![
            Reply::org(),
            Reply::json(json!({"dashboard":{"uid":"board-1","large":value},"meta":{}})),
        ]);
        if expansion {
            fixture.provider["auth"]["token"] = json!("e");
        }
        let (output, result) = fixture.run(&mut fixture.dashboard());
        assert_eq!(output.status.code(), Some(1), "{}", result["errors"]);
        assert!(
            has_error(
                &result,
                if expansion {
                    "redaction_output_too_large"
                } else {
                    "structured_response_too_large"
                }
            ),
            "{}",
            result["errors"]
        );
        assert_eq!(result["data"]["response"], Value::Null);
        assert!(output.stdout.len() <= 2_097_152);
        assert_eq!(result["coverage"]["state"], "not_established");
    }
}

#[test]
fn individual_request_deadline_is_twenty_seconds_within_the_sixty_second_operation() {
    let mut reply = Reply::org();
    reply.header_delay = Duration::from_secs(25);
    let fixture = Fixture::new(vec![reply, Reply::dashboard()]);
    let started = Instant::now();
    let (output, result) = fixture.run(&mut fixture.dashboard());
    assert_eq!(output.status.code(), Some(1), "{result}");
    assert_eq!(result["execution"]["status"], "timed_out");
    assert!(has_error(&result, "deadline_exceeded"));
    assert_eq!(fixture.requests().len(), 1);
    assert_eq!(result["effective_limits"]["timeout_ms"], 60_000);
    assert!((Duration::from_secs(19)..Duration::from_secs(23)).contains(&started.elapsed()));
}

#[test]
fn review_r1_finalized_receipt_includes_late_source_and_timing_overhead() {
    const MAX: usize = 2_097_152;
    let observe = |padding: usize| {
        let fixture = Fixture::new(vec![
            Reply::org(),
            Reply::json(
                json!({"dashboard":{"uid":"board-1","padding":"z".repeat(padding)},"meta":{}}),
            ),
        ]);
        fixture.run(&mut fixture.dashboard())
    };
    let baseline_padding = MAX - 5_000;
    let (baseline, result) = observe(baseline_padding);
    assert_eq!(baseline.status.code(), Some(0), "{}", result["errors"]);
    assert!(baseline.stdout.len() < MAX);
    // Calibrate the complete envelope using a real accepted response. 32 bytes is
    // larger than variable timestamp/PID widths but smaller than the late source.
    let padding = baseline_padding + MAX + 32 - baseline.stdout.len();
    let (output, result) = observe(padding);
    assert_eq!(result["effective_limits"]["max_output_bytes"], MAX);
    assert!(
        output.stdout.len() <= MAX,
        "final receipt has {} bytes, cap {MAX}; padding {padding}",
        output.stdout.len()
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        has_error(&result, "structured_response_too_large"),
        "{}",
        result["errors"]
    );
    assert_eq!(result["data"]["response"], Value::Null);
    assert_eq!(result["coverage"]["state"], "not_established");
}

#[test]
fn review_r1_canonical_origin_length_is_checked_before_http() {
    for (count, accepted) in [(400, false), (300, true)] {
        let mut fixture = Fixture::new(vec![Reply::org(), Reply::dashboard()]);
        let origin = format!(
            "{}/{}",
            fixture.provider["origin"].as_str().unwrap(),
            "é".repeat(count)
        );
        assert!(origin.chars().count() < 2048);
        fixture.provider["origin"] = json!(origin);
        fixture.edit_config(|config| config["connections"]["fixture"]["origin"] = json!(origin));
        let (output, result) = fixture.run(&mut fixture.dashboard());
        if accepted {
            assert_eq!(output.status.code(), Some(0), "{}", result["errors"]);
            assert!(result["data"]["binding"]["origin"].as_str().unwrap().len() <= 2048);
            assert_eq!(fixture.requests().len(), 2);
        } else {
            assert_eq!(
                output.status.code(),
                Some(2),
                "canonical origin length {}, dispatched {} requests",
                result["data"]["binding"]["origin"]
                    .as_str()
                    .map_or(0, str::len),
                fixture.requests().len()
            );
            assert!(has_error(&result, "invalid_origin"));
            assert_eq!(result["effect_outcome"], "not_attempted");
            assert_eq!(result["data"]["binding"], Value::Null);
            assert!(fixture.requests().is_empty());
        }
    }
}

#[test]
fn review_r1_fractional_time_precision_is_admitted_before_lossy_parser_conversion() {
    for (fraction, accepted) in [
        ("0000000001", false),
        ("1230000001", false),
        ("00000000000000000001", false),
        ("0001", false),
        ("1230000000000", true),
        ("0000000000000", true),
    ] {
        let fixture = Fixture::new(vec![
            Reply::org(),
            Reply::datasource("prometheus"),
            Reply::query(),
        ]);
        let from = format!("2026-10-02T00:00:00.{fraction}Z");
        let (output, result) = fixture.run(fixture.command().args([
            "grafana",
            "query",
            "--target",
            "fixture",
            "--datasource",
            "metrics-1",
            "--kind",
            "prometheus",
            "--from",
            &from,
            "--to",
            "2026-10-02T00:01:00Z",
            "--expr",
            "up",
        ]));
        if accepted {
            assert_eq!(output.status.code(), Some(0), "{}", result["errors"]);
            let normalized = if fraction.starts_with("123") {
                "2026-10-02T00:00:00.123Z"
            } else {
                "2026-10-02T00:00:00Z"
            };
            assert_eq!(result["data"]["request"]["from"], normalized);
            assert_eq!(fixture.requests().len(), 3);
        } else {
            assert_eq!(
                output.status.code(),
                Some(2),
                "fraction {fraction} normalized to {}, dispatched {} requests",
                result["data"]["request"]["from"],
                fixture.requests().len()
            );
            assert!(has_error(&result, "invalid_grafana_input"));
            assert_eq!(result["effect_outcome"], "not_attempted");
            assert!(fixture.requests().is_empty());
        }
    }
}

#[test]
fn review_r2_invalid_timestamp_separators_cannot_bypass_exact_precision_admission() {
    let mut failures = Vec::new();
    for (field, supplied) in [
        ("from", "2026-10-02T00:00:00.0000000001Z"),
        ("from", "2026-10-02.00:00:00.0000000001Z"),
        ("from", "2026-10-02.00:00:00Z"),
        ("from", "2026-10-02 00:00:00Z"),
        ("from", "2026-10-02_00:00:00.1230000000000+00:00"),
        ("to", "2026-10-02.00:01:00.0000000001Z"),
        ("to", "2026-10-02:00:01:00Z"),
    ] {
        let fixture = Fixture::new(vec![
            Reply::org(),
            Reply::datasource("prometheus"),
            Reply::query(),
        ]);
        let from = if field == "from" {
            supplied
        } else {
            "2026-10-02T00:00:00Z"
        };
        let to = if field == "to" {
            supplied
        } else {
            "2026-10-02T00:01:00Z"
        };
        let (output, result) = fixture.run(fixture.command().args([
            "grafana",
            "query",
            "--target",
            "fixture",
            "--datasource",
            "metrics-1",
            "--kind",
            "prometheus",
            "--from",
            from,
            "--to",
            to,
            "--expr",
            "up",
        ]));
        if output.status.code() != Some(2)
            || !has_error(&result, "invalid_grafana_input")
            || !fixture.requests().is_empty()
            || result["effect_outcome"] != "not_attempted"
        {
            failures.push(format!(
                "{field}={supplied:?}: exit {:?}, dispatched {}, normalized {}",
                output.status.code(),
                fixture.requests().len(),
                result["data"]["request"][field]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for supplied in [
        "2026-10-02t00:00:00.1230000000000z",
        "2026-10-02T01:00:00.1230000000000+01:00",
        "2026-10-01t23:00:00.1230000000000-01:00",
    ] {
        let fixture = Fixture::new(vec![
            Reply::org(),
            Reply::datasource("prometheus"),
            Reply::query(),
        ]);
        let (output, result) = fixture.run(fixture.command().args([
            "grafana",
            "query",
            "--target",
            "fixture",
            "--datasource",
            "metrics-1",
            "--kind",
            "prometheus",
            "--from",
            supplied,
            "--to",
            "2026-10-02t00:01:00.0000000000000z",
            "--expr",
            "up",
        ]));
        assert_eq!(
            output.status.code(),
            Some(0),
            "{supplied}: {}",
            result["errors"]
        );
        assert_eq!(
            result["data"]["request"]["from"],
            "2026-10-02T00:00:00.123Z"
        );
        assert_eq!(result["data"]["request"]["to"], "2026-10-02T00:01:00Z");
        let requests = fixture.requests();
        assert_eq!(requests.len(), 3);
        let body: Value = serde_json::from_slice(&requests[2].body).unwrap();
        assert_eq!(body["from"], "1790899200123");
        assert_eq!(body["to"], "1790899260000");
    }
}
