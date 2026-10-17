//! Fixed legacy Grafana exchanges. Fixture conformance is not live-version certification.
mod config;
mod redact;
pub use config::{
    GrafanaConfigDescription, GrafanaConfigIdentity, GrafanaTargetDescription,
    grafana_config_description,
};

use crate::{
    OperatorContext, RunControl, queries,
    request::{MAX_RESULT_BYTES, Problem, Request, parse_bounded_json},
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
pub(crate) fn date(value: &str) -> Option<(String, i64)> {
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
    query_input(&request.inputs)
}

fn query_input(value: &Value) -> Result<Input, Error> {
    let invalid = || Error::admission("invalid_grafana_input");
    let mut input: Query = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
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

fn admitted_input(
    request: &Request,
    started_at: &str,
) -> Result<(Input, Option<queries::Compiled>), Error> {
    if request.operation != "query.run" {
        return Ok((input(request)?, None));
    }
    let compiled = queries::compile(&request.inputs, started_at).map_err(|problem| {
        if problem.unsupported {
            Error::unsupported(problem.code)
        } else {
            Error::admission(problem.code)
        }
    })?;
    // Retain every legacy guard, including macros inside a label literal, before
    // configuration or HTTP work. This is input normalization, not a nested run.
    Ok((query_input(&compiled.input)?, Some(compiled)))
}

fn collection_problem(error: Error) -> Problem {
    let message = "The evidence collection's Grafana binding or query could not be admitted; private configuration and query details are withheld.";
    if error.status == Status::Unsupported {
        Problem::unsupported(error.code, message)
    } else {
        Problem::invalid(error.code, message)
    }
}

/// Exercise compilation and every legacy query guard before any context/config/store read.
pub(crate) fn prepare_collection_query(
    inputs: &Value,
    started_at: &str,
) -> Result<queries::Compiled, Problem> {
    let compiled = queries::compile(inputs, started_at)?;
    query_input(&compiled.input).map_err(collection_problem)?;
    Ok(compiled)
}

/// Private admission state. No provider value, credential or original expression is
/// serializable; callers can retain only the explicitly sanitized metadata projection.
pub(crate) struct CollectionBinding {
    pub identity: GrafanaConfigIdentity,
    pub organization: String,
    pub connection: Value,
    pub config_sha256: Value,
    pub ca: Value,
    redactor: Redactor,
}

impl CollectionBinding {
    pub fn metadata(&mut self, value: Value) -> Value {
        self.redactor.metadata(value)
    }

    pub fn redaction(&self) -> Value {
        json!({"applied":self.redactor.replacements > 0,"replacements":self.redactor.replacements})
    }
}

pub(crate) fn prepare_collection_binding(
    context: &OperatorContext,
    alias: &str,
) -> Result<CollectionBinding, Problem> {
    let path = context
        .config_path
        .as_deref()
        .ok_or_else(|| collection_problem(Error::admission("configuration_required")))?;
    let (description, identity) = grafana_config_description(path)?;
    // A changed file between description and admission is refused on the exact bytes
    // used by the existing provider; the same identity is then supplied to every child.
    let prepared = config::load(Some(path), alias, "grafana.query", Some(&identity))
        .map_err(collection_problem)?;
    let mut redactor = redactor(&prepared.secrets, true).map_err(collection_problem)?;
    let connection = json!({"id":alias,"origin":redactor.metadata(json!(prepared.origin)),
        "organization_id":redactor.metadata(json!(prepared.organization))});
    let config_sha256 = redactor.metadata(json!(description.digest));
    let ca = match identity.ca_digest(alias) {
        Some(digest) => json!({"kind":"custom","sha256":redactor.metadata(json!(digest))}),
        None => json!({"kind":"builtin","sha256":null}),
    };
    Ok(CollectionBinding {
        identity,
        organization: prepared.organization.to_string(),
        connection,
        config_sha256,
        ca,
        redactor,
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
    #[cfg(target_os = "linux")]
    if let Some(fd) = control.result_fd {
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: one initialized descriptor observation is borrowed without blocking.
        if unsafe { libc::poll(&mut poll, 1, 0) } > 0
            && poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
        {
            return Err(Error {
                code: "result_consumer_closed",
                status: Status::Cancelled,
            });
        }
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

fn metadata(input: &Input, redactor: &mut Redactor, private_job: Option<&Value>) -> Value {
    match input {
        Input::Dashboard(input) => json!({"uid":redactor.metadata(json!(input.uid))}),
        Input::Query {
            input, interval, ..
        } => {
            // Literal encoding inserts backslashes into quotes/backslashes in a job.
            // Keep the source value's sensitivity even if its escaped expression no
            // longer contains the original credential as a contiguous substring.
            let expr = if private_job.is_some_and(Value::is_object) {
                redactor.replacements += 1;
                json!({"redacted":true})
            } else {
                redactor.metadata(json!(input.expr))
            };
            json!({
                "datasource":redactor.metadata(json!(input.datasource)),"kind":redactor.metadata(json!(input.kind)),
                "from":redactor.metadata(json!(input.from)),"to":redactor.metadata(json!(input.to)),"expr":expr,
                "max_data_points":1000,"interval_ms":interval,"max_lines":if input.kind == "loki" {Some(500)} else {None}
            })
        }
    }
}

fn redactor(secrets: &[String], saved_query: bool) -> Result<Redactor, Error> {
    if !saved_query {
        return Redactor::new(secrets);
    }
    let mut variants = secrets.to_vec();
    // Grafana can echo the compiled expression in remote frame metadata. Add exactly
    // the compiler's known literal transformation before masking metadata and payloads.
    // No expression or original parameter copy is appended after this redactor runs.
    variants.extend(secrets.iter().map(|secret| queries::escaped_label(secret)));
    Redactor::new(&variants)
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
    expected_identity: Option<&GrafanaConfigIdentity>,
    start: Instant,
    result: &mut OperationResult,
) {
    let deadline = start + Duration::from_millis(request.limits.timeout_ms.min(60_000));
    run_with_deadline(
        request,
        control,
        context,
        expected_identity,
        start,
        deadline,
        result,
    );
}

pub(crate) fn run_with_deadline(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    expected_identity: Option<&GrafanaConfigIdentity>,
    start: Instant,
    parent_deadline: Instant,
    result: &mut OperationResult,
) {
    result.effective_limits.timeout_ms = request.limits.timeout_ms.min(60_000);
    result.effective_limits.max_output_bytes =
        request.limits.max_output_bytes.min(MAX_RESULT_BYTES);
    let child_deadline = start + Duration::from_millis(result.effective_limits.timeout_ms);
    let deadline = parent_deadline.min(child_deadline);
    result.effective_limits.timeout_ms = result.effective_limits.timeout_ms.min(
        (deadline
            .saturating_duration_since(start)
            .as_millis()
            .min(u128::from(u64::MAX)) as u64)
            .max(1),
    );
    result.data = json!({"adapter":{"id":"grafana","version":1,"api_profile":PROFILE,"compatibility_evidence":"fixture_only","upstream_revision":"7d900fb999abee3b3712e1da881dcc7e80b1f6e7","upstream_digest":"sha256:1c1711d8bd1c70e49e5715813506a0a4389021041f457d9d6b1a95a39f8fc3f7"},"binding":null,"request":null,"response":null,"redaction":{"applied":false,"replacements":0},"transport":{"stage":"admission","requests_started":0,"responses_received":0,"request_timeout_ms":20000,"response_limit_bytes":result.effective_limits.max_output_bytes},"cancellation_signal":null});
    if request.operation == "query.run" {
        result.data["definition"] = Value::Null;
        result.data["parameters"] = Value::Null;
        result.data["window"] = Value::Null;
    }
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
        let (input, saved) = admitted_input(request, &result.started_at)?;
        if let Some(saved) = &saved {
            result.data["definition"] = saved.definition.clone();
            result
                .coverage
                .limitations
                .extend(saved.limitations.clone());
        }
        let permission = if saved.is_some() {
            "grafana.query"
        } else {
            &request.operation
        };
        let prepared = config::load(
            context.config_path.as_deref(),
            &request.target.id,
            permission,
            expected_identity,
        )?;
        check(control, deadline)?;
        let mut redactor = redactor(&prepared.secrets, saved.is_some())?;
        result.resolved_target = json!({"connection":request.target.id,"profile":"operator-local","api_profile":PROFILE,"effects":"observe"});
        result.data["binding"] = json!({"origin":redactor.metadata(json!(prepared.origin)),"organization_id":redactor.metadata(json!(prepared.organization)),"organization_verified":false});
        let private_job = saved
            .as_ref()
            .map(|saved| redactor.metadata(json!(saved.job)));
        result.data["request"] = metadata(&input, &mut redactor, private_job.as_ref());
        if let Some(saved) = saved {
            result.data["parameters"] = json!({"job":private_job.expect("compiled saved job")});
            result.data["window"] = json!({"from":redactor.metadata(saved.input["from"].clone()),"to":redactor.metadata(saved.input["to"].clone())});
        }
        result.data["redaction"] =
            json!({"applied":redactor.replacements > 0,"replacements":redactor.replacements});
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

    fn saved_request(job: &str) -> Request {
        Request::new(
            "query.run",
            json!({"id":"prometheus.up-by-job","version":1,"datasource":"metrics-1",
                "parameters":{"job":job},"window":{"kind":"relative","lookback_seconds":300}}),
        )
    }

    #[test]
    fn collection_keeps_the_absolute_parent_deadline_before_admission() {
        let request = saved_request("fixture");
        let mut result = OperationResult::new(&request);
        let start = Instant::now();
        run_with_deadline(
            &request,
            &RunControl::default(),
            &OperatorContext::default(),
            None,
            start,
            start - Duration::from_millis(1),
            &mut result,
        );
        assert_eq!(result.execution.status, Status::TimedOut);
        assert_eq!(result.data["transport"]["requests_started"], 0);
        assert!(result.data["definition"].is_null());
        assert_eq!(result.effective_limits.timeout_ms, 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn closed_result_consumer_cancels_network_admission() {
        use std::os::{fd::AsRawFd, unix::net::UnixStream};
        let (writer, reader) = UnixStream::pair().unwrap();
        drop(reader);
        let control = RunControl {
            result_fd: Some(writer.as_raw_fd()),
            ..RunControl::default()
        };
        let error = check(&control, Instant::now() + Duration::from_secs(1)).unwrap_err();
        assert_eq!(error.code, "result_consumer_closed");
        assert_eq!(error.status, Status::Cancelled);
    }

    #[test]
    fn saved_query_admission_keeps_legacy_macro_and_datasource_guards() {
        for job in ["$__interval", "${job}", "[[service]]", "quote\"$__interval"] {
            let request = saved_request(job);
            assert_eq!(
                admitted_input(&request, "2026-10-10T00:00:00.123456Z")
                    .err()
                    .unwrap()
                    .code,
                "unresolved_expression"
            );
        }
        let mut request = saved_request("checkout$");
        assert!(admitted_input(&request, "2026-10-10T00:00:00.123456Z").is_ok());
        request.inputs["datasource"] = json!("../other");
        assert_eq!(
            admitted_input(&request, "2026-10-10T00:00:00.123456Z")
                .err()
                .unwrap()
                .code,
            "invalid_grafana_input"
        );
    }

    #[test]
    fn saved_escaped_credential_source_masks_the_compiled_literal() {
        for job in ["credential\"quote", "credential\\slash", "雪\"back\\slash"] {
            let request = saved_request(job);
            let (input, saved) = admitted_input(&request, "2026-10-10T00:00:00Z").unwrap();
            let saved = saved.unwrap();
            let mut redactor = redactor(&[job.into()], true).unwrap();
            let private_job = redactor.metadata(json!(saved.job));
            assert_eq!(private_job, json!({"redacted":true}));
            let metadata = metadata(&input, &mut redactor, Some(&private_job));
            assert_eq!(metadata["expr"], json!({"redacted":true}));
            assert_eq!(metadata["max_data_points"], 1000);
            assert_eq!(saved.definition["id"], "prometheus.up-by-job");
            assert_eq!(redactor.replacements, 2);
            let response = redactor
                .payload(json!({"executedQueryString":saved.input["expr"]}))
                .unwrap();
            assert_eq!(response["executedQueryString"], "up{job=\"[REDACTED]\"}");
            assert_eq!(redactor.replacements, 3);
        }
    }

    #[test]
    fn saved_admission_and_config_failures_do_not_echo_private_inputs() {
        for (job, has_definition) in [("private-canary", true), ("$__interval", false)] {
            let request = saved_request(job);
            let mut result = OperationResult::new(&request);
            run(
                &request,
                &RunControl::default(),
                &OperatorContext::default(),
                None,
                Instant::now(),
                &mut result,
            );
            assert_eq!(result.data["transport"]["requests_started"], 0);
            assert!(result.data["parameters"].is_null());
            assert!(result.data["window"].is_null());
            assert!(result.data["request"].is_null());
            assert_eq!(result.data["definition"].is_object(), has_definition);
            assert!(!result.data.to_string().contains(job));
        }
    }

    #[test]
    fn saved_query_uses_the_existing_parent_deadline_and_signal() {
        let request = saved_request("fixture");
        let mut result = OperationResult::new(&request);
        run(
            &request,
            &RunControl::default(),
            &OperatorContext::default(),
            None,
            Instant::now() - Duration::from_secs(61),
            &mut result,
        );
        assert_eq!(result.execution.status, Status::TimedOut);
        assert_eq!(result.data["transport"]["requests_started"], 0);
        assert!(result.data["definition"].is_null());
        let mut result = OperationResult::new(&request);
        let control = RunControl::default();
        control.signal.store(15, Ordering::Relaxed);
        run(
            &request,
            &control,
            &OperatorContext::default(),
            None,
            Instant::now(),
            &mut result,
        );
        assert_eq!(result.execution.status, Status::Cancelled);
        assert_eq!(result.data["cancellation_signal"], 15);
        assert_eq!(result.exit_code(), 143);
        assert_eq!(result.data["transport"]["requests_started"], 0);
    }

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
