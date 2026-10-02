//! Fixed legacy Grafana exchanges. Fixture conformance is not live-version certification.
mod config;
mod redact;

use crate::{
    OperatorContext, RunControl,
    request::{MAX_RESULT_BYTES, Request, parse_bounded_json},
    result::{OperationResult, Status, timestamp},
};
use config::Prepared;
use redact::Redactor;
use reqwest::{
    Client, Method,
    dns::{Addrs, Name, Resolve, Resolving},
    header,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

const PROFILE: &str = "grafana-legacy-v1";

#[derive(Debug)]
struct Error {
    code: &'static str,
    status: Status,
}
impl Error {
    fn admission(code: &'static str) -> Self {
        Self {
            code,
            status: Status::Denied,
        }
    }
    fn unsupported(code: &'static str) -> Self {
        Self {
            code,
            status: Status::Unsupported,
        }
    }
    fn failed(code: &'static str) -> Self {
        Self {
            code,
            status: Status::Failed,
        }
    }
    fn timeout() -> Self {
        Self {
            code: "deadline_exceeded",
            status: Status::TimedOut,
        }
    }
    fn cancelled() -> Self {
        Self {
            code: "cancelled",
            status: Status::Cancelled,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dashboard {
    uid: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    datasource: String,
    kind: String,
    from: String,
    to: String,
    expr: String,
}
enum Input {
    Dashboard(Dashboard),
    Query {
        input: Query,
        from: i64,
        to: i64,
        interval: i64,
    },
}

fn uid(value: &str) -> bool {
    (1..=40).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn date(value: &str) -> Option<(String, i64)> {
    if value.len() > 64 {
        return None;
    }
    // time accepts arbitrary date/time separators and truncates fractions beyond
    // nanoseconds. Admit the RFC3339 grammar and exact milliseconds before parsing.
    static TIMESTAMP: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"\A[0-9]{4}-[0-9]{2}-[0-9]{2}[Tt][0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.([0-9]+))?(?:[Zz]|[+-][0-9]{2}:[0-9]{2})\z",
        )
        .expect("fixed RFC3339 expression")
    });
    let parts = TIMESTAMP.captures(value)?;
    if parts
        .get(1)
        .is_some_and(|fraction| fraction.as_str().bytes().skip(3).any(|digit| digit != b'0'))
    {
        return None;
    }
    let parsed = OffsetDateTime::parse(value, &Rfc3339).ok()?;
    let nanos = parsed.unix_timestamp_nanos();
    if nanos < 0 || nanos % 1_000_000 != 0 {
        return None;
    }
    let millis = i64::try_from(nanos / 1_000_000).ok()?;
    if millis > 253_402_300_799_999 {
        return None;
    }
    Some((
        parsed.to_offset(UtcOffset::UTC).format(&Rfc3339).ok()?,
        millis,
    ))
}
fn input(request: &Request) -> Result<Input, Error> {
    let invalid = || Error::admission("invalid_grafana_input");
    if request.operation == "grafana.dashboard.get" {
        let input: Dashboard =
            serde_json::from_value(request.inputs.clone()).map_err(|_| invalid())?;
        if !uid(&input.uid) {
            return Err(invalid());
        }
        return Ok(Input::Dashboard(input));
    }
    let mut input: Query = serde_json::from_value(request.inputs.clone()).map_err(|_| invalid())?;
    if !uid(&input.datasource)
        || !matches!(input.kind.as_str(), "prometheus" | "loki")
        || input.expr.trim().is_empty()
        || input.expr.chars().count() > 16000
        || input.expr.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    static MACRO: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\$(?:[A-Za-z_]|\{)|\[\[[A-Za-z_][^\]]*\]\]")
            .expect("fixed macro expression")
    });
    if MACRO.is_match(&input.expr) {
        return Err(Error::admission("unresolved_expression"));
    }
    let (from_text, from) = date(&input.from).ok_or_else(invalid)?;
    let (to_text, to) = date(&input.to).ok_or_else(invalid)?;
    if from >= to || to - from > 86_400_000 {
        return Err(invalid());
    }
    input.from = from_text;
    input.to = to_text;
    Ok(Input::Query {
        input,
        from,
        to,
        interval: 1000.max((to - from + 999) / 1000),
    })
}

// Reqwest's default Hickory wrapper falls back to public DNS when system configuration
// fails. This resolver initializes only for names and fails closed instead. IP URLs bypass it.
struct SystemDns;
impl Resolve for SystemDns {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let mut builder = hickory_resolver::TokioResolver::builder_tokio()?;
            builder.options_mut().ip_strategy =
                hickory_resolver::config::LookupIpStrategy::Ipv6AndIpv4;
            let resolver = builder.build()?;
            let lookup = resolver.lookup_ip(name.as_str()).await?;
            let addresses = lookup
                .iter()
                .map(|ip| SocketAddr::new(ip, 0))
                .collect::<Vec<_>>();
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

fn client(prepared: &Prepared) -> Result<Client, Error> {
    Client::builder()
        .tls_backend_rustls()
        .tls_certs_only(prepared.roots.clone())
        .tls_sslkeylogfile(false)
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .referer(false)
        .http1_only()
        .dns_resolver(Arc::new(SystemDns))
        .pool_max_idle_per_host(0)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| Error::failed("transport_unavailable"))
}

fn check(control: &RunControl, deadline: Instant) -> Result<(), Error> {
    if control.signal.load(Ordering::Relaxed) != 0 {
        return Err(Error::cancelled());
    }
    if Instant::now() >= deadline {
        return Err(Error::timeout());
    }
    Ok(())
}

struct Exchange<'a> {
    client: Client,
    prepared: &'a Prepared,
    control: &'a RunControl,
    deadline: Instant,
    maximum: usize,
}
impl Exchange<'_> {
    async fn read(
        &self,
        stage: &'static str,
        path: &str,
        body: Option<Value>,
        result: &mut OperationResult,
    ) -> Result<Value, Error> {
        check(self.control, self.deadline)?;
        result.data["transport"]["stage"] = json!(stage);
        let mut request = self
            .client
            .request(
                if body.is_some() {
                    Method::POST
                } else {
                    Method::GET
                },
                format!("{}{path}", self.prepared.origin),
            )
            .header(header::AUTHORIZATION, self.prepared.authorization.clone())
            .header("X-Grafana-Org-Id", self.prepared.organization.to_string())
            .header(header::ACCEPT, "application/json")
            .header(header::ACCEPT_ENCODING, "identity")
            .timeout(
                Duration::from_secs(20)
                    .min(self.deadline.saturating_duration_since(Instant::now())),
            );
        if let Some(body) = body {
            request = request
                .header(header::CONTENT_TYPE, "application/json")
                .body(body.to_string());
        }
        check(self.control, self.deadline)?;
        result.effect_outcome = "not_applicable".into();
        let count = result.data["transport"]["requests_started"]
            .as_u64()
            .unwrap_or(0);
        result.data["transport"]["requests_started"] = json!(count + 1);
        let operation = async {
            let mut response = request.send().await.map_err(transport_error)?;
            let count = result.data["transport"]["responses_received"]
                .as_u64()
                .unwrap_or(0);
            result.data["transport"]["responses_received"] = json!(count + 1);
            if response.status().is_redirection() {
                return Err(Error::failed("redirect_rejected"));
            }
            if !response.status().is_success() {
                return Err(Error::failed("http_error"));
            }
            if response
                .headers()
                .get(header::CONTENT_ENCODING)
                .is_some_and(|v| v != "identity")
            {
                return Err(Error::failed("unsupported_content_encoding"));
            }
            if response
                .content_length()
                .is_some_and(|size| size > self.maximum as u64)
            {
                return Err(Error::failed("response_too_large"));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
                if chunk.len() > self.maximum.saturating_sub(bytes.len()) {
                    return Err(Error::failed("response_too_large"));
                }
                bytes.extend_from_slice(&chunk);
                check(self.control, self.deadline)?;
            }
            let value =
                parse_bounded_json(&bytes, 64).map_err(|_| Error::failed("invalid_response"))?;
            if !value.is_object() {
                return Err(Error::failed("invalid_response"));
            }
            check(self.control, self.deadline)?;
            Ok(value)
        };
        let cancelled = async {
            loop {
                check(self.control, self.deadline)?;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            #[allow(unreachable_code)]
            Ok::<(), Error>(())
        };
        tokio::select! {
            biased;
            error = cancelled => { error?; Err(Error::cancelled()) },
            value = operation => value,
        }
    }
}
fn transport_error(error: reqwest::Error) -> Error {
    // Error's Display/Debug may contain URLs or private data. Never forward either.
    if error.is_timeout() {
        Error::timeout()
    } else {
        Error::failed("request_failed")
    }
}

fn metadata(input: &Input, redactor: &mut Redactor) -> Value {
    match input {
        Input::Dashboard(input) => json!({"uid":redactor.metadata(json!(input.uid))}),
        Input::Query {
            input, interval, ..
        } => json!({
            "datasource":redactor.metadata(json!(input.datasource)),"kind":redactor.metadata(json!(input.kind)),
            "from":redactor.metadata(json!(input.from)),"to":redactor.metadata(json!(input.to)),"expr":redactor.metadata(json!(input.expr)),
            "max_data_points":1000,"interval_ms":interval,"max_lines":if input.kind == "loki" {Some(500)} else {None}
        }),
    }
}

async fn observe(
    exchange: Exchange<'_>,
    input: Input,
    result: &mut OperationResult,
) -> Result<Value, Error> {
    let organization = exchange
        .read("organization", "/api/org", None, result)
        .await?;
    if organization["id"].as_i64() != Some(exchange.prepared.organization) {
        return Err(Error::failed("organization_mismatch"));
    }
    result.data["binding"]["organization_verified"] = json!(true);
    match input {
        Input::Dashboard(input) => {
            let mut response = exchange
                .read(
                    "dashboard",
                    &format!("/api/dashboards/uid/{}", input.uid),
                    None,
                    result,
                )
                .await?;
            if !response["dashboard"].is_object()
                || response["dashboard"]["uid"].as_str() != Some(&input.uid)
                || !response["meta"].is_object()
            {
                return Err(Error::failed("invalid_dashboard_response"));
            }
            Ok(json!({"dashboard":response["dashboard"].take(),"meta":response["meta"].take()}))
        }
        Input::Query {
            input,
            from,
            to,
            interval,
        } => {
            let source = exchange
                .read(
                    "datasource",
                    &format!(
                        "/api/datasources/uid/{}?ds_type={}",
                        input.datasource, input.kind
                    ),
                    None,
                    result,
                )
                .await?;
            if source
                .get("orgId")
                .is_some_and(|org| org.as_i64() != Some(exchange.prepared.organization))
            {
                return Err(Error::failed("organization_mismatch"));
            }
            if source["uid"].as_str() != Some(&input.datasource)
                || source["type"].as_str() != Some(&input.kind)
            {
                return Err(Error::failed("datasource_mismatch"));
            }
            let mut query = json!({"refId":"A","datasource":{"uid":input.datasource,"type":input.kind},"expr":input.expr,"maxDataPoints":1000,"intervalMs":interval});
            if input.kind == "prometheus" {
                query["range"] = json!(true);
                query["instant"] = json!(false);
                query["format"] = json!("time_series");
            } else {
                query["queryType"] = json!("range");
                query["maxLines"] = json!(500);
            }
            let mut response = exchange
                .read(
                    "query",
                    "/api/ds/query",
                    Some(json!({"from":from.to_string(),"to":to.to_string(),"queries":[query]})),
                    result,
                )
                .await?;
            let results = &response["results"];
            let result_a = &results["A"];
            let no_error =
                |v: Option<&Value>| v.is_none_or(|v| v.is_null() || v.as_str() == Some(""));
            let status_valid = match result_a.get("status") {
                None => true,
                Some(Value::Number(n)) => n.as_u64().is_some_and(|v| (100..400).contains(&v)),
                Some(Value::String(s)) => {
                    s.len() == 3
                        && s.bytes().all(|b| b.is_ascii_digit())
                        && s.parse::<u64>().is_ok_and(|v| (100..400).contains(&v))
                }
                _ => false,
            };
            if !no_error(response.get("error"))
                || !results
                    .as_object()
                    .is_some_and(|m| m.len() == 1 && m.contains_key("A"))
                || !result_a.is_object()
                || !no_error(result_a.get("error"))
                || !status_valid
                || !result_a["frames"].is_array()
            {
                return Err(Error::failed("query_failed"));
            }
            Ok(json!({"results":response["results"].take()}))
        }
    }
}

pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    result.effective_limits.timeout_ms = request.limits.timeout_ms.min(60_000);
    result.effective_limits.max_output_bytes =
        request.limits.max_output_bytes.min(MAX_RESULT_BYTES);
    let deadline = start + Duration::from_millis(result.effective_limits.timeout_ms);
    result.data = json!({"adapter":{"id":"grafana","version":1,"api_profile":PROFILE,"compatibility_evidence":"fixture_only","upstream_revision":"7d900fb999abee3b3712e1da881dcc7e80b1f6e7","upstream_digest":"sha256:1c1711d8bd1c70e49e5715813506a0a4389021041f457d9d6b1a95a39f8fc3f7"},"binding":null,"request":null,"response":null,"redaction":{"applied":false,"replacements":0},"transport":{"stage":"admission","requests_started":0,"responses_received":0,"request_timeout_ms":20000,"response_limit_bytes":result.effective_limits.max_output_bytes},"cancellation_signal":null});
    result.coverage.state = "not_established".into();
    result.coverage.scope = if request.operation == "grafana.dashboard.get" {
        "Selected dashboard configuration from the configured Grafana connection"
    } else {
        "Bounded query response from the configured Grafana connection and datasource"
    }
    .into();
    result.coverage.limitations = vec![
        "This adapter has fixture evidence only; no live Grafana version, rendering, panel transformation or service-health claim is made.".into(),
        "Known credentials are masked in private echoes and observational data; arbitrary sensitive telemetry is not detected. Public request identity and generated controls are preserved.".into(),
        "Requested points/lines do not prove returned coverage. Empty frames are not an all-clear; cancelling a client does not prove remote query cancellation.".into(),
        "The operator environment provider catches accidental retargeting; it does not isolate credentials from the same OS account.".into(),
    ];
    let outcome = (|| {
        if !cfg!(target_os = "linux") {
            return Err(Error::unsupported("unsupported_platform"));
        }
        check(control, deadline)?;
        let input = input(request)?;
        let prepared = config::load(
            context.config_path.as_deref(),
            &request.target.id,
            &request.operation,
        )?;
        check(control, deadline)?;
        let mut redactor = Redactor::new(&prepared.secrets)?;
        result.resolved_target = json!({"connection":request.target.id,"profile":"operator-local","api_profile":PROFILE,"effects":"observe"});
        result.data["binding"] = json!({"origin":redactor.metadata(json!(prepared.origin)),"organization_id":redactor.metadata(json!(prepared.organization)),"organization_verified":false});
        result.data["request"] = metadata(&input, &mut redactor);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| Error::failed("transport_unavailable"))?;
        let operation = runtime.block_on(async {
            let client = client(&prepared)?;
            let exchange = Exchange {
                client,
                prepared: &prepared,
                control,
                deadline,
                maximum: result.effective_limits.max_output_bytes,
            };
            observe(exchange, input, result).await
        });
        // Async requests, resolver and client have been dropped; no background blocking DNS exists.
        runtime.shutdown_timeout(Duration::from_millis(100));
        let response = operation.and_then(|mut response| {
            check(control, deadline)?;
            // These wrapper keys/refId are generated protocol controls. Only the remote
            // dashboard/meta or A contents enter the private-data redactor.
            if request.operation == "grafana.dashboard.get" {
                Ok(json!({"dashboard":redactor.payload(response["dashboard"].take())?,"meta":redactor.payload(response["meta"].take())?}))
            } else {
                Ok(json!({"results":{"A":redactor.payload(response["results"]["A"].take())?}}))
            }
        });
        result.data["redaction"] =
            json!({"applied":redactor.replacements > 0,"replacements":redactor.replacements});
        result.data["response"] = response?;
        check(control, deadline)?;
        if serde_json::to_vec(&result)
            .map_err(|_| Error::failed("result_encoding_failed"))?
            .len()
            + 1
            > MAX_RESULT_BYTES
        {
            return Err(Error::failed("structured_response_too_large"));
        }
        check(control, deadline)?;
        result.data["transport"]["stage"] = json!("complete");
        if request.operation == "grafana.dashboard.get" {
            result.coverage.state = "complete".into();
        }
        result.sources.push(json!({"reference":"grafana:selected-connection","observed_at":timestamp(),"evidence_state":"sourced","untrusted":true}));
        Ok(())
    })();
    if let Err(error) = outcome {
        result.data["response"] = Value::Null;
        result.execution.status = error.status;
        result.coverage.state = "not_established".into();
        // Static, intentionally general prose. The precise machine code identifies the failing gate.
        result.error(error.code, "The Grafana observation could not complete. Check the named configuration, supplied input, connection and operation limits; private response and transport details are withheld.");
    }
    let signal = control.signal.load(Ordering::Relaxed);
    if signal != 0 {
        result.data["cancellation_signal"] = json!(signal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_r2_timestamp_grammar_and_fraction_precision_are_independent() {
        for separator in 0_u8..=127 {
            let value = format!("2026-10-02{}00:00:00Z", char::from(separator));
            assert_eq!(
                date(&value).is_some(),
                matches!(separator, b'T' | b't'),
                "separator byte {separator}"
            );
        }
        for separator in ["T", "t"] {
            for zone in ["Z", "z", "+00:00", "-00:00", "+05:30", "-01:00"] {
                assert!(
                    date(&format!(
                        "2026-10-02{separator}00:00:00.1230000000000{zone}"
                    ))
                    .is_some()
                );
                assert!(
                    date(&format!(
                        "2026-10-02{separator}00:00:00.1230000000001{zone}"
                    ))
                    .is_none()
                );
            }
        }
        for digits in 4..=43 {
            let zero_tail = format!("123{}", "0".repeat(digits - 3));
            let nonzero_tail = format!("123{}1", "0".repeat(digits - 4));
            assert_eq!(
                date(&format!("2026-10-02T00:00:00.{zero_tail}Z")),
                date("2026-10-02T00:00:00.123Z")
            );
            assert!(
                date(&format!("2026-10-02T00:00:00.{nonzero_tail}Z")).is_none(),
                "fraction length {digits}"
            );
        }
        for invalid in [
            "2026-10-02T00:00:00.Z",
            "2026-10-02T00:00:00Z\n",
            "2026-10-02TT00:00:00Z",
            "2026-10-02T00:00:00+0100",
            "2026-10-02T00:00:00.000+01:00.0",
            "2026-10-02T00:00:00,123Z",
            "2026-13-02T00:00:00Z",
            "2026-10-02T24:00:00Z",
        ] {
            assert!(date(invalid).is_none(), "{invalid:?}");
        }
    }
    #[test]
    fn timestamps_and_expressions_preserve_the_public_time_contract() {
        assert_eq!(
            date("2026-10-02T01:00:00+01:00"),
            date("2026-10-02T00:00:00Z")
        );
        assert!(date("2026-10-02T00:00:00.000001Z").is_none());
        assert!(date("1969-12-31T23:59:59Z").is_none());
        let base = json!({"datasource":"metrics-1","kind":"prometheus","from":"2026-10-02T00:00:00Z","to":"2026-10-02T00:01:00Z","expr":"up{job=~\"checkout$\"}"});
        assert!(input(&Request::new("grafana.query", base.clone())).is_ok());
        for expression in ["$__interval", "[[service]]", "${region}", "up\n", " "] {
            let mut value = base.clone();
            value["expr"] = json!(expression);
            assert!(input(&Request::new("grafana.query", value)).is_err());
        }
    }
}
