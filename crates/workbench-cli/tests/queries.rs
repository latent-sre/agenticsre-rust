#![cfg(all(feature = "test-fixtures", target_os = "linux"))]
#[allow(dead_code)]
#[path = "support/grafana_https.rs"]
mod https;

use https::*;
use serde_json::{Value, json};
use std::{
    fs,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const PROMETHEUS: &str = "prometheus.up-by-job";
const LOKI: &str = "loki.logs-by-job";

fn absolute_window() -> Value {
    json!({
        "kind":"absolute",
        "from":"2026-10-02T05:30:00.1230000000000+05:30",
        "to":"2026-10-02T01:01:00.123-00:00"
    })
}

fn query_command(fixture: &Fixture, id: &str, parameters: &Value, window: &Value) -> Command {
    let parameters_file = fixture.directory.join("parameters.json");
    let window_file = fixture.directory.join("window.json");
    fs::write(&parameters_file, parameters.to_string()).unwrap();
    fs::write(&window_file, window.to_string()).unwrap();
    let mut command = fixture.command();
    command
        .args([
            "queries",
            "run",
            id,
            "--version",
            "1",
            "--target",
            "fixture",
            "--datasource",
            "metrics-1",
            "--parameters",
        ])
        .arg(parameters_file)
        .arg("--window")
        .arg(window_file);
    command
}

fn describe(fixture: &Fixture, id: &str) -> Value {
    let (output, result) =
        fixture.run(
            fixture
                .command()
                .args(["queries", "describe", id, "--version", "1"]),
        );
    assert_eq!(output.status.code(), Some(0), "{result}");
    result["data"]["query"].clone()
}

fn inputs() -> Value {
    json!({"id":PROMETHEUS,"version":1,"datasource":"metrics-1","parameters":{"job":"fixture"},"window":absolute_window()})
}

fn request(inputs: Value) -> Value {
    json!({"spec_version":"0.1","request_id":"saved-query-fixture","operation":"query.run","operation_version":1,"target":{"kind":"connection","id":"fixture"},"inputs":inputs,"limits":{"timeout_ms":60000,"max_output_bytes":2097152},"record":"never"})
}

fn call_command(fixture: &Fixture, request: &str) -> Command {
    let path = fixture.directory.join("request.json");
    fs::write(&path, request).unwrap();
    let mut command = fixture.command();
    command.args(["call", "--request"]).arg(path);
    command
}

fn stable_receipt(mut result: Value) -> Value {
    let result_map = result.as_object_mut().unwrap();
    for key in [
        "request_id",
        "run_id",
        "started_at",
        "finished_at",
        "duration_ms",
    ] {
        assert!(result_map.remove(key).is_some(), "missing {key}");
    }
    for source in result["sources"].as_array_mut().unwrap() {
        source.as_object_mut().unwrap().remove("observed_at");
    }
    result
}

// Independently convert the receipt's UTC RFC3339 timestamp to an epoch millisecond.
// This deliberately does not call the production window parser or sample the clock.
fn receipt_millis(timestamp: &str) -> i64 {
    assert!(timestamp.ends_with('Z'), "{timestamp}");
    let year: i64 = timestamp[0..4].parse().unwrap();
    let month: usize = timestamp[5..7].parse().unwrap();
    let day: i64 = timestamp[8..10].parse().unwrap();
    let hour: i64 = timestamp[11..13].parse().unwrap();
    let minute: i64 = timestamp[14..16].parse().unwrap();
    let second: i64 = timestamp[17..19].parse().unwrap();
    let leap = |year: i64| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let mut days: i64 = (1970..year)
        .map(|year| if leap(year) { 366 } else { 365 })
        .sum();
    let lengths = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    days += lengths[..month - 1].iter().sum::<i64>() + day - 1;
    let fractional = timestamp[19..].strip_prefix('.').unwrap_or("");
    let millis = fractional
        .bytes()
        .take_while(u8::is_ascii_digit)
        .chain(std::iter::repeat(b'0'))
        .take(3)
        .fold(0, |total, digit| total * 10 + i64::from(digit - b'0'));
    ((days * 24 + hour) * 60 * 60 + minute * 60 + second) * 1000 + millis
}

#[test]
fn installed_queries_use_exact_scoped_promql_and_logql_exchanges() {
    let fixture = Fixture::new(vec![
        Reply::org(),
        Reply::datasource("prometheus"),
        Reply::query(),
        Reply::org(),
        Reply::datasource("loki"),
        Reply::query(),
    ]);
    let job = "café\\worker\"},other=\"value\"} or vector(1) #";
    let literal = r#""café\\worker\"},other=\"value\"} or vector(1) #""#;
    for (index, id, kind, dialect, metric) in [
        (0, PROMETHEUS, "prometheus", "promql", "up"),
        (3, LOKI, "loki", "logql", ""),
    ] {
        let installed = describe(&fixture, id);
        assert_eq!(fixture.requests().len(), index);
        let digest = installed["definition_sha256"].as_str().unwrap();
        assert_eq!(digest.len(), 64);
        assert!(
            digest
                .bytes()
                .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
        );
        let expression = format!("{metric}{{job={literal}}}");
        let (output, result) = fixture.run(
            query_command(&fixture, id, &json!({"job":job}), &absolute_window())
                .args(["--expected-definition-sha256", digest]),
        );
        assert_eq!(output.status.code(), Some(0), "{result}");
        assert_eq!(result["operation"], "query.run");
        assert_eq!(result["operation_version"], 1);
        assert_eq!(result["assessment"], "not_assessed");
        assert_eq!(result["record_mode"], "never");
        assert_eq!(result["coverage"]["state"], "not_established");
        assert_eq!(result["execution"]["child_exit_code"], Value::Null);
        assert_eq!(result["data"]["cancellation_signal"], Value::Null);
        assert_eq!(
            result["data"]["definition"],
            json!({"id":id,"version":1,"dialect":dialect,"sha256":digest})
        );
        assert_eq!(result["data"]["parameters"], json!({"job":job}));
        assert_eq!(
            result["data"]["window"],
            json!({"from":"2026-10-02T00:00:00.123Z","to":"2026-10-02T01:01:00.123Z"})
        );
        assert_eq!(result["data"]["request"]["expr"], expression);
        assert_eq!(
            result["data"]["response"],
            json!({"results":{"A":{"frames":[]}}})
        );
        let requests = fixture.requests();
        assert_eq!(requests.len(), index + 3);
        assert_eq!(requests[index].method, "GET");
        assert_eq!(requests[index].path, "/grafana/api/org");
        assert_eq!(requests[index + 1].method, "GET");
        assert_eq!(
            requests[index + 1].path,
            format!("/grafana/api/datasources/uid/metrics-1?ds_type={kind}")
        );
        assert_eq!(requests[index + 2].method, "POST");
        assert_eq!(requests[index + 2].path, "/grafana/api/ds/query");
        for request in &requests[index..] {
            assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
            assert_eq!(request.headers["x-grafana-org-id"], "7");
        }
        let mut expected_query = json!({"refId":"A","datasource":{"uid":"metrics-1","type":kind},"expr":expression,"maxDataPoints":1000,"intervalMs":3660});
        if kind == "prometheus" {
            expected_query["range"] = json!(true);
            expected_query["instant"] = json!(false);
            expected_query["format"] = json!("time_series");
        } else {
            expected_query["queryType"] = json!("range");
            expected_query["maxLines"] = json!(500);
        }
        let body: Value = serde_json::from_slice(&requests[index + 2].body).unwrap();
        assert_eq!(
            body,
            json!({"from":"1790899200123","to":"1790902860123","queries":[expected_query]})
        );
    }
}

#[test]
fn discovery_is_installed_local_metadata_without_configuration_or_credentials() {
    let fixture = Fixture::new(vec![]);
    let missing = fixture.directory.join("missing-operator.json");
    let mut command = Command::new(SAVE);
    command
        .env_clear()
        .env(
            "SAVE_GRAFANA_FIXTURE",
            "this is deliberately not a provider",
        )
        .args(["--json", "--config"])
        .arg(&missing)
        .args(["queries", "list"]);
    let (output, listed) = fixture.run(&mut command);
    assert_eq!(output.status.code(), Some(0), "{listed}");
    assert_eq!(listed["operation"], "queries.list");
    assert_eq!(listed["target"], json!({"kind":"local","id":"workstation"}));
    let entries = listed["data"]["queries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    for id in [PROMETHEUS, LOKI] {
        let entry = entries.iter().find(|entry| entry["id"] == id).unwrap();
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .args(["--json", "--config"])
            .arg(&missing)
            .args(["queries", "describe", id, "--version", "1"]);
        let (output, described) = fixture.run(&mut command);
        assert_eq!(output.status.code(), Some(0), "{described}");
        assert_eq!(described["operation"], "queries.describe");
        assert_eq!(&described["data"]["query"], entry);
        assert_eq!(entry["version"], 1);
        assert_eq!(entry["definition_sha256"].as_str().unwrap().len(), 64);
    }
    assert!(!missing.exists());
    assert!(fixture.requests().is_empty());
}

#[test]
fn human_query_receipt_escapes_remote_controls_and_shows_identity_and_coverage() {
    let fixture = Fixture::new(vec![
        Reply::org(),
        Reply::datasource("prometheus"),
        Reply::json(json!({"results":{"A":{"frames":[{"line":"remote\u{9b}31mtext"}]}}})),
    ]);
    let query = query_command(
        &fixture,
        PROMETHEUS,
        &json!({"job":"fixture"}),
        &absolute_window(),
    );
    let mut command = Command::new(SAVE);
    command
        .env_clear()
        .env("SAVE_GRAFANA_FIXTURE", fixture.provider.to_string())
        .args(query.get_args().filter(|value| *value != "--json"));
    let output = command.output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        !text.contains('\u{9b}'),
        "unescaped C1 control in text receipt"
    );
    assert!(text.contains("\\u{9b}"));
    for expected in [
        "Definition:",
        PROMETHEUS,
        "Parameters:",
        "fixture",
        "Window:",
        "2026-10-02T00:00:00.123Z",
        "Observation:",
        "Coverage: not_established",
        "Limit:",
    ] {
        assert!(text.contains(expected), "missing {expected} in {text}");
    }
    assert_eq!(fixture.requests().len(), 3);
}

#[test]
fn direct_and_structured_saved_queries_preserve_the_same_complete_receipt() {
    for (id, kind) in [(PROMETHEUS, "prometheus"), (LOKI, "loki")] {
        let fixture = Fixture::new(vec![
            Reply::org(),
            Reply::datasource(kind),
            Reply::query(),
            Reply::org(),
            Reply::datasource(kind),
            Reply::query(),
        ]);
        let (direct_output, direct) = fixture.run(
            query_command(&fixture, id, &json!({"job":"fixture"}), &absolute_window()).args([
                "--timeout",
                "60s",
                "--max-output-bytes",
                "2097152",
            ]),
        );
        let mut supplied = inputs();
        supplied["id"] = json!(id);
        let (call_output, called) =
            fixture.run(&mut call_command(&fixture, &request(supplied).to_string()));
        assert_eq!(direct_output.status.code(), Some(0), "{direct}");
        assert_eq!(call_output.status.code(), Some(0), "{called}");
        assert_eq!(called["request_id"], "saved-query-fixture");
        assert_eq!(stable_receipt(called), stable_receipt(direct));
        let requests = fixture.requests();
        assert_eq!(requests.len(), 6);
        assert_eq!(requests[2].body, requests[5].body);
    }
}

#[test]
fn relative_window_uses_parent_start_once_despite_slow_binding_checks() {
    let mut organization = Reply::org();
    organization.header_delay = Duration::from_millis(140);
    let mut datasource = Reply::datasource("prometheus");
    datasource.header_delay = Duration::from_millis(140);
    let fixture = Fixture::new(vec![organization, datasource, Reply::query()]);
    let (output, result) = fixture.run(&mut query_command(
        &fixture,
        PROMETHEUS,
        &json!({"job":"relative-anchor"}),
        &json!({"kind":"relative","lookback_seconds":300}),
    ));
    assert_eq!(output.status.code(), Some(0), "{result}");
    let anchor = receipt_millis(result["started_at"].as_str().unwrap());
    assert_eq!(
        receipt_millis(result["data"]["window"]["to"].as_str().unwrap()),
        anchor
    );
    assert_eq!(
        receipt_millis(result["data"]["window"]["from"].as_str().unwrap()),
        anchor - 300_000
    );
    assert_eq!(
        result["data"]["request"]["from"],
        result["data"]["window"]["from"]
    );
    assert_eq!(
        result["data"]["request"]["to"],
        result["data"]["window"]["to"]
    );
    let requests = fixture.requests();
    assert_eq!(requests.len(), 3);
    let body: Value = serde_json::from_slice(&requests[2].body).unwrap();
    assert_eq!(body["to"], anchor.to_string());
    assert_eq!(body["from"], (anchor - 300_000).to_string());
    assert!(result["duration_ms"].as_u64().unwrap() >= 250);
}

#[test]
fn strict_saved_query_admission_rejects_invalid_inputs_before_http() {
    let fixture = Fixture::new(vec![]);
    for patch in [
        json!({"id":"not-installed"}),
        json!({"version":2}),
        json!({"version":null}),
        json!({"dialect":"promql"}),
        json!({"expression":"up"}),
        json!({"catalog_path":"/tmp/queries.json"}),
        json!({"datasource":"metrics-1/../../other"}),
        json!({"parameters":null}),
        json!({"parameters":{}}),
        json!({"parameters":{"job":"fixture","other":"forbidden"}}),
        json!({"parameters":{"job":null}}),
        json!({"parameters":{"job":true}}),
        json!({"parameters":{"job":{"type":"string"}}}),
        json!({"parameters":{"job":""}}),
        json!({"parameters":{"job":"é".repeat(65)}}),
        json!({"parameters":{"job":"line\nbreak"}}),
        json!({"parameters":{"job":"control\u{0085}character"}}),
        json!({"parameters":{"job":"$job"}}),
        json!({"parameters":{"job":"${job}"}}),
        json!({"parameters":{"job":"[[job]]"}}),
        json!({"window":null}),
        json!({"window":{"kind":"now","lookback_seconds":30}}),
        json!({"window":{"kind":"relative","lookback_seconds":0}}),
        json!({"window":{"kind":"relative","lookback_seconds":86401}}),
        json!({"window":{"kind":"relative","lookback_seconds":1.5}}),
        json!({"window":{"kind":"relative","lookback_seconds":true}}),
        json!({"window":{"kind":"relative","lookback_seconds":null}}),
        json!({"window":{"kind":"relative","lookback_seconds":30,"to":"now"}}),
        json!({"window":{"kind":"absolute","from":"2026-10-02T00:00:00Z","to":"2026-10-02T00:00:00Z"}}),
        json!({"window":{"kind":"absolute","from":"2026-10-02T00:00:00Z","to":"2026-10-03T00:00:00.001Z"}}),
        json!({"window":{"kind":"absolute","from":"2026-10-02T00:00:00.0000000001Z","to":"2026-10-02T00:01:00Z"}}),
        json!({"window":{"kind":"absolute","from":"2026-10-02.00:00:00Z","to":"2026-10-02T00:01:00Z"}}),
        json!({"window":{"kind":"absolute","from":null,"to":"2026-10-02T00:01:00Z"}}),
        json!({"expected_definition_sha256":null}),
        json!({"expected_definition_sha256":"0".repeat(64)}),
        json!({"expected_definition_sha256":"A".repeat(64)}),
        json!({"expected_definition_sha256":"0".repeat(63)}),
    ] {
        let mut supplied = inputs();
        for (key, value) in patch.as_object().unwrap() {
            supplied[key] = value.clone();
        }
        let (output, result) =
            fixture.run(&mut call_command(&fixture, &request(supplied).to_string()));
        assert_eq!(output.status.code(), Some(2), "patch {patch}: {result}");
        assert!(
            matches!(
                result["execution"]["status"].as_str(),
                Some("denied" | "unsupported")
            ),
            "{result}"
        );
        assert!(!result["errors"].as_array().unwrap().is_empty(), "{result}");
        assert_eq!(result["effect_outcome"], "not_attempted");
        assert!(fixture.requests().is_empty(), "patch {patch}");
    }
    for (which, raw) in [
        ("parameters.json", r#"{"job":"fixture","job":"duplicate"}"#),
        ("parameters.json", "null"),
        ("parameters.json", r#"{"job":"fixture","unknown":true}"#),
        (
            "window.json",
            r#"{"kind":"relative","kind":"absolute","lookback_seconds":30}"#,
        ),
        (
            "window.json",
            r#"{"kind":"relative","lookback_seconds":30,"lookback_seconds":60}"#,
        ),
        ("window.json", "null"),
    ] {
        let mut command = query_command(
            &fixture,
            PROMETHEUS,
            &json!({"job":"fixture"}),
            &absolute_window(),
        );
        fs::write(fixture.directory.join(which), raw).unwrap();
        let (output, result) = fixture.run(&mut command);
        assert_eq!(output.status.code(), Some(2), "{which} {raw}: {result}");
        assert_eq!(result["effect_outcome"], "not_attempted");
        assert!(fixture.requests().is_empty());
    }
    let duplicated = request(inputs()).to_string().replace(
        "\"job\":\"fixture\"",
        "\"job\":\"fixture\",\"job\":\"duplicate\"",
    );
    let (output, result) = fixture.run(&mut call_command(&fixture, &duplicated));
    assert_eq!(output.status.code(), Some(2), "{result}");
    assert!(fixture.requests().is_empty());
}

#[test]
fn utf8_parameter_and_time_window_boundaries_are_actual_supported_requests() {
    for window in [
        json!({"kind":"relative","lookback_seconds":1}),
        json!({"kind":"relative","lookback_seconds":86400}),
        json!({"kind":"absolute","from":"2026-10-02T00:00:00Z","to":"2026-10-03T00:00:00Z"}),
    ] {
        let fixture = Fixture::new(vec![
            Reply::org(),
            Reply::datasource("prometheus"),
            Reply::query(),
        ]);
        let job = "é".repeat(64);
        let (output, result) = fixture.run(&mut query_command(
            &fixture,
            PROMETHEUS,
            &json!({"job":job}),
            &window,
        ));
        assert_eq!(output.status.code(), Some(0), "{window}: {result}");
        assert_eq!(result["data"]["parameters"]["job"], job);
        let requests = fixture.requests();
        assert_eq!(requests.len(), 3);
        let body: Value = serde_json::from_slice(&requests[2].body).unwrap();
        let from: i64 = body["from"].as_str().unwrap().parse().unwrap();
        let to: i64 = body["to"].as_str().unwrap().parse().unwrap();
        let expected = window["lookback_seconds"].as_i64().unwrap_or(86400) * 1000;
        assert_eq!(to - from, expected);
    }
}

#[test]
fn saved_query_credentials_cover_private_inputs_compiled_metadata_and_responses() {
    for token in [
        TOKEN,
        "synthetic\"token\\secret",
        "2026-10-02",
        PROMETHEUS,
        "true",
    ] {
        let escaped = token.replace('\\', "\\\\").replace('"', "\\\"");
        let compiled_expression = format!("up{{job=\"{escaped}\"}}");
        let mut fixture = Fixture::new(vec![
            Reply::org(),
            Reply::datasource("prometheus"),
            Reply::json(
                json!({"results":{"A":{"frames":[{"echo":token,"schema":{"meta":{"executedQueryString":compiled_expression}}}]}}}),
            ),
        ]);
        fixture.provider["auth"]["token"] = json!(token);
        let installed = describe(&fixture, PROMETHEUS);
        let (output, result) = fixture.run(&mut query_command(
            &fixture,
            PROMETHEUS,
            &json!({"job":token}),
            &absolute_window(),
        ));
        assert_eq!(output.status.code(), Some(0), "{result}");
        assert_eq!(
            result["data"]["definition"],
            json!({"id":PROMETHEUS,"version":1,"dialect":"promql","sha256":installed["definition_sha256"]})
        );
        assert_eq!(
            result["data"]["parameters"]["job"],
            json!({"redacted":true})
        );
        assert_eq!(result["data"]["request"]["expr"], json!({"redacted":true}));
        assert_eq!(
            result["data"]["response"]["results"]["A"]["frames"][0]["echo"],
            "[REDACTED]"
        );
        let echoed_expression = result["data"]["response"]["results"]["A"]["frames"][0]["schema"]["meta"]["executedQueryString"].as_str().unwrap();
        assert!(
            echoed_expression.contains("[REDACTED]"),
            "{echoed_expression}"
        );
        assert!(
            !echoed_expression.contains(&escaped),
            "escaped credential leaked in {echoed_expression}"
        );
        assert_eq!(result["data"]["redaction"]["applied"], true);
        if token == "2026-10-02" {
            assert_eq!(
                result["data"]["window"],
                json!({"from":{"redacted":true},"to":{"redacted":true}})
            );
            assert_eq!(result["data"]["request"]["from"], json!({"redacted":true}));
            assert_eq!(result["data"]["request"]["to"], json!({"redacted":true}));
        }
        if token == TOKEN {
            assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
        }
        let requests = fixture.requests();
        assert_eq!(requests.len(), 3);
        assert!(
            requests
                .iter()
                .all(|request| request.headers["authorization"] == format!("Bearer {token}"))
        );
        let body: Value = serde_json::from_slice(&requests[2].body).unwrap();
        assert_eq!(body["queries"][0]["expr"], compiled_expression);
    }
    let fixture = Fixture::new(vec![]);
    let mut supplied = inputs();
    supplied["parameters"] = json!({"job":TOKEN,"unknown":TOKEN});
    let (output, result) = fixture.run(&mut call_command(&fixture, &request(supplied).to_string()));
    assert_eq!(output.status.code(), Some(2), "{result}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
    assert!(fixture.requests().is_empty());
}

#[test]
fn query_run_reuses_only_the_existing_grafana_read_permission() {
    for permitted in [false, true] {
        let mut fixture = Fixture::new(vec![
            Reply::org(),
            Reply::datasource("prometheus"),
            Reply::query(),
        ]);
        fixture.provider["operations"] = if permitted {
            json!(["grafana.query"])
        } else {
            json!(["query.run"])
        };
        let (output, result) = fixture.run(&mut query_command(
            &fixture,
            PROMETHEUS,
            &json!({"job":"fixture"}),
            &absolute_window(),
        ));
        assert_eq!(
            output.status.code(),
            Some(if permitted { 0 } else { 2 }),
            "{result}"
        );
        assert_eq!(fixture.requests().len(), if permitted { 3 } else { 0 });
        if !permitted {
            assert_eq!(result["effect_outcome"], "not_attempted");
        }
    }
}

#[test]
fn binding_tls_and_http200_query_failures_retain_identity_and_stop_dispatch() {
    for (replies, count) in [
        (vec![Reply::json(json!({"id":8}))], 1),
        (vec![Reply::org(), Reply::datasource("loki")], 2),
        (
            vec![
                Reply::org(),
                Reply::json(json!({"uid":"metrics-1","type":"prometheus","orgId":8})),
            ],
            2,
        ),
        (
            vec![
                Reply::org(),
                Reply::datasource("prometheus"),
                Reply::json(json!({"results":{"A":{"error":TOKEN,"frames":[]}}})),
            ],
            3,
        ),
        (
            vec![
                Reply::org(),
                Reply::datasource("prometheus"),
                Reply::json(json!({"results":{"A":{"status":403,"frames":[]}}})),
            ],
            3,
        ),
    ] {
        let fixture = Fixture::new(replies);
        let (output, result) = fixture.run(&mut query_command(
            &fixture,
            PROMETHEUS,
            &json!({"job":"fixture"}),
            &absolute_window(),
        ));
        assert_eq!(output.status.code(), Some(1), "{result}");
        assert_eq!(result["execution"]["status"], "failed");
        assert!(!result["errors"].as_array().unwrap().is_empty());
        assert_eq!(result["data"]["definition"]["id"], PROMETHEUS);
        assert_eq!(result["data"]["response"], Value::Null);
        assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
        assert_eq!(fixture.requests().len(), count);
    }
    let fixture = Fixture::new(vec![Reply::org()]);
    fixture.edit_config(|config| {
        config["connections"]["fixture"]
            .as_object_mut()
            .unwrap()
            .remove("tls_ca_file");
    });
    let (output, result) = fixture.run(
        query_command(
            &fixture,
            PROMETHEUS,
            &json!({"job":"fixture"}),
            &absolute_window(),
        )
        .env("SSL_CERT_FILE", &fixture.ca),
    );
    assert_eq!(output.status.code(), Some(1), "{result}");
    assert_eq!(result["data"]["definition"]["id"], PROMETHEUS);
    assert!(fixture.requests().is_empty());
}

#[test]
fn empty_and_backend_partial_observations_preserve_payload_without_claiming_coverage() {
    for returned in [
        json!({"results":{"A":{"frames":[]}}}),
        json!({"results":{"A":{"status":206,"frames":[{"schema":{"meta":{"notices":[{"severity":"warning","text":"partial result"}]}},"data":{"values":[]}}]}}}),
    ] {
        let fixture = Fixture::new(vec![
            Reply::org(),
            Reply::datasource("prometheus"),
            Reply::json(returned.clone()),
        ]);
        let (output, result) = fixture.run(&mut query_command(
            &fixture,
            PROMETHEUS,
            &json!({"job":"fixture"}),
            &absolute_window(),
        ));
        assert_eq!(output.status.code(), Some(0), "{result}");
        assert_eq!(result["data"]["response"], returned);
        assert_eq!(result["assessment"], "not_assessed");
        assert_eq!(result["coverage"]["state"], "not_established");
        assert!(
            !result["coverage"]["limitations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(fixture.requests().len(), 3);
    }
}

#[test]
fn saved_query_deadline_is_shared_across_binding_and_body_consumption() {
    let mut org = Reply::org();
    org.header_delay = Duration::from_millis(90);
    let mut datasource = Reply::datasource("prometheus");
    datasource.header_delay = Duration::from_millis(90);
    let mut query = Reply::query();
    query.body_delay = Duration::from_millis(250);
    let fixture = Fixture::new(vec![org, datasource, query, Reply::query()]);
    let started = Instant::now();
    let (output, result) = fixture.run(
        query_command(
            &fixture,
            PROMETHEUS,
            &json!({"job":"fixture"}),
            &absolute_window(),
        )
        .args(["--timeout", "300ms"]),
    );
    assert_eq!(output.status.code(), Some(1), "{result}");
    assert_eq!(result["execution"]["status"], "timed_out");
    assert_eq!(result["effective_limits"]["timeout_ms"], 300);
    assert_eq!(result["data"]["definition"]["id"], PROMETHEUS);
    assert_eq!(result["data"]["response"], Value::Null);
    assert_eq!(fixture.requests().len(), 3);
    assert!(started.elapsed() < Duration::from_secs(2));
    thread::sleep(Duration::from_millis(40));
    assert_eq!(fixture.requests().len(), 3);
}

#[test]
fn active_sigint_and_sigterm_preserve_signal_and_stop_all_later_requests() {
    for (signal, active_stage, count) in [
        (libc::SIGINT, "organization", 1),
        (libc::SIGTERM, "query", 3),
    ] {
        let mut org = Reply::org();
        let mut query = Reply::query();
        if active_stage == "organization" {
            org.body_delay = Duration::from_secs(3);
        } else {
            query.body_delay = Duration::from_secs(3);
        }
        let fixture = Fixture::new(vec![
            org,
            Reply::datasource("prometheus"),
            query,
            Reply::query(),
        ]);
        let mut child = query_command(
            &fixture,
            PROMETHEUS,
            &json!({"job":"fixture"}),
            &absolute_window(),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
        fixture.wait_for_requests(count);
        let started = Instant::now();
        // SAFETY: this PID is the still-owned CLI child, whose request is active in the private fixture.
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        while child.try_wait().unwrap().is_none() {
            if started.elapsed() > Duration::from_secs(2) {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("saved query did not stop after signal {signal}");
            }
            thread::sleep(Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap();
        let result = receipt(&output);
        assert_eq!(output.status.code(), Some(128 + signal), "{result}");
        assert_eq!(result["execution"]["status"], "cancelled");
        assert_eq!(result["data"]["cancellation_signal"], signal);
        assert_eq!(result["data"]["definition"]["id"], PROMETHEUS);
        assert_eq!(result["data"]["response"], Value::Null);
        assert_eq!(fixture.requests().len(), count);
        thread::sleep(Duration::from_millis(40));
        assert_eq!(fixture.requests().len(), count);
        // Drop joins the server worker even while its programmed response remains delayed.
        drop(fixture);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

#[test]
fn bounded_response_and_final_saved_query_receipt_keep_definition_when_omitted() {
    const MAX: usize = 2_097_152;
    let observe = |padding: usize, limit: Option<&str>| {
        let fixture = Fixture::new(vec![
            Reply::org(),
            Reply::datasource("prometheus"),
            Reply::json(json!({"results":{"A":{"frames":[{"padding":"z".repeat(padding)}]}}})),
        ]);
        let mut command = query_command(
            &fixture,
            PROMETHEUS,
            &json!({"job":"fixture"}),
            &absolute_window(),
        );
        if let Some(limit) = limit {
            command.args(["--max-output-bytes", limit]);
        }
        let result = fixture.run(&mut command);
        assert_eq!(fixture.requests().len(), 3);
        result
    };
    let (output, result) = observe(5000, Some("4096"));
    assert_eq!(output.status.code(), Some(1), "{result}");
    assert_eq!(result["data"]["response"], Value::Null);
    assert_eq!(result["data"]["definition"]["id"], PROMETHEUS);
    assert_eq!(result["effective_limits"]["max_output_bytes"], 4096);
    let baseline_padding = MAX - 20_000;
    let (baseline, result) = observe(baseline_padding, None);
    assert_eq!(baseline.status.code(), Some(0), "{}", result["errors"]);
    assert!(baseline.stdout.len() < MAX);
    // Include the complete finalized metadata, source, timing and trailing newline.
    let padding = baseline_padding + MAX + 32 - baseline.stdout.len();
    let (output, result) = observe(padding, None);
    assert_eq!(output.status.code(), Some(1), "{}", result["errors"]);
    assert!(
        output.stdout.len() <= MAX,
        "complete receipt has {} bytes",
        output.stdout.len()
    );
    assert_eq!(result["data"]["response"], Value::Null);
    assert_eq!(result["data"]["definition"]["id"], PROMETHEUS);
    assert_eq!(
        result["data"]["definition"]["sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(result["coverage"]["state"], "not_established");
}
