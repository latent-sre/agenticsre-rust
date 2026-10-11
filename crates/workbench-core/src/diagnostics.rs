//! Fixed connectivity DAG. Configuration binds destinations; results never grant authority.
mod config;
mod dns;
mod http;
mod process;

use crate::{
    OperatorContext, RunControl,
    request::{Problem, Request},
    result::{OperationResult, Status, timestamp},
};
use config::{Prepared, Resolver};
use hickory_resolver::proto::rr::RecordType;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    future::Future,
    net::{IpAddr, SocketAddr},
    path::Path,
    pin::Pin,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, client::TlsStream};

const IDS: [&str; 5] = ["dns", "tcp", "tls", "http", "process"];
const OPERATIONS: [&str; 5] = [
    "network.dns.lookup",
    "network.tcp.connect",
    "network.tls.handshake",
    "network.http.get",
    "local.process.observe",
];
const STEP_LIMIT_MS: u64 = 10_000;

/// Explicit trusted hosted authority, admitted from regular configuration and selected CA bytes.
/// Fields stay private: a wire request cannot manufacture or expand this grant.
pub struct DiagnosticsGrant {
    configuration_sha256: String,
    targets: BTreeMap<String, Value>,
}

impl DiagnosticsGrant {
    pub fn target_ids(&self) -> impl Iterator<Item = &str> {
        self.targets.keys().map(String::as_str)
    }

    fn validate(&self, target_id: &str, identity: &Value) -> Result<(), Problem> {
        let ca = self
            .targets
            .get(target_id)
            .ok_or_else(|| config::problem("diagnostics_target_not_granted"))?;
        if identity["configuration_sha256"] != self.configuration_sha256
            || &identity["additional_ca_sha256"] != ca
        {
            return Err(config::problem("diagnostics_identity_changed"));
        }
        Ok(())
    }
}

/// Admit only explicitly selected aliases, without resolver, socket or process observations.
/// Every invocation rereads and compares the exact admitted configuration/CA bytes before dispatch.
pub fn diagnostics_grant(path: &Path, target_ids: &[String]) -> Result<DiagnosticsGrant, Problem> {
    if target_ids.is_empty() || target_ids.len() > 16 {
        return Err(config::problem("invalid_diagnostics_grant"));
    }
    let control = RunControl::default();
    let deadline = Instant::now() + Duration::from_millis(STEP_LIMIT_MS);
    let mut grant = DiagnosticsGrant {
        configuration_sha256: String::new(),
        targets: BTreeMap::new(),
    };
    for id in target_ids {
        if grant.targets.contains_key(id) {
            return Err(config::problem("invalid_diagnostics_grant"));
        }
        let prepared = config::load(Some(path), id, &control, deadline)?;
        let configuration_sha256 = prepared.identity["configuration_sha256"]
            .as_str()
            .expect("admitted configuration identity");
        if !grant.configuration_sha256.is_empty()
            && grant.configuration_sha256 != configuration_sha256
        {
            return Err(config::problem("diagnostics_identity_changed"));
        }
        grant.configuration_sha256 = configuration_sha256.into();
        grant.targets.insert(
            id.clone(),
            prepared.identity["additional_ca_sha256"].clone(),
        );
    }
    Ok(grant)
}

/// Fixed installed metadata only; discovery performs no target observations.
pub fn diagnostics_pack_metadata() -> Value {
    metadata()
}

/// Normalize the same fixed typed pack input used by execution, for hosted deduplication.
pub fn normalize_diagnostics_input(inputs: &Value) -> Result<Value, Problem> {
    let input = input(inputs)?;
    Ok(json!({"id":input.id,"version":input.version,"input":{
        "capture_body":input.input.capture_body,"on_failure":input.input.on_failure}}))
}

pub(super) fn digest(bytes: &[u8]) -> String {
    let mut value = String::from("sha256:");
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write;
        write!(value, "{byte:02x}").expect("string writing");
    }
    value
}

pub(super) fn check(control: &RunControl, deadline: Instant) -> Result<(), Problem> {
    if control.signal.load(Ordering::Relaxed) != 0 {
        return Err(config::problem("cancelled"));
    }
    if Instant::now() >= deadline {
        return Err(config::problem("deadline_exceeded"));
    }
    #[cfg(target_os = "linux")]
    if let Some(fd) = control.result_fd {
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: one initialized pollfd is borrowed for a nonblocking descriptor observation.
        if unsafe { libc::poll(&mut poll, 1, 0) } > 0
            && poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
        {
            return Err(config::problem("result_consumer_closed"));
        }
    }
    Ok(())
}

pub(crate) fn metadata() -> Value {
    let steps = IDS.iter().enumerate().map(|(index, id)| json!({"id":id,"operation":OPERATIONS[index],"operation_version":1,
        "prerequisites":if (1..4).contains(&index) {vec![IDS[index - 1]]} else {vec![]},"max_timeout_ms":STEP_LIMIT_MS})).collect::<Vec<_>>();
    let definition = json!({"id":"connectivity","version":1,"steps":steps,"max_concurrency":2,"max_timeout_ms":60000,
        "input_schema":{"$ref":"urn:sre-workbench:spec:diagnostics-pack-input:0.1"},
        "output_schema":{"$ref":"urn:sre-workbench:spec:diagnostics-data:0.1"},"effects":"observe","platforms":["linux"]});
    let identity = digest(&serde_json::to_vec(&definition).expect("fixed definition"));
    json!({"definition":definition,"sha256":identity})
}

pub(crate) fn list() -> Value {
    json!({"packs":[{"pack":metadata(),"availability":crate::discovery::availability("diagnostics.run")}]})
}

pub(crate) fn describe(inputs: &Value, result: &mut OperationResult) {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        id: String,
    }
    match serde_json::from_value::<Input>(inputs.clone()) {
        Ok(input) if input.id == "connectivity" => {
            result.effect_outcome = "not_applicable".into();
            result.data = json!({"pack":metadata(),"availability":crate::discovery::availability("diagnostics.run")});
        }
        Ok(_) => result.reject(Problem::unsupported(
            "unknown_diagnostics_pack",
            "Only the fixed connectivity version-1 pack is installed.",
        )),
        Err(_) => result.reject(config::problem("invalid_diagnostics_input")),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackInput {
    #[serde(default)]
    capture_body: bool,
    #[serde(default = "continue_mode")]
    on_failure: String,
}
fn continue_mode() -> String {
    "continue".into()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    id: String,
    version: u64,
    input: PackInput,
}

fn input(inputs: &Value) -> Result<Input, Problem> {
    if !inputs.is_object() || !inputs["input"].is_object() {
        return Err(config::problem("invalid_diagnostics_input"));
    }
    let input: Input = serde_json::from_value(inputs.clone())
        .map_err(|_| config::problem("invalid_diagnostics_input"))?;
    if input.id != "connectivity" || input.version != 1 {
        return Err(Problem::unsupported(
            "unknown_diagnostics_pack",
            "Only the fixed connectivity version-1 pack is installed.",
        ));
    }
    if !matches!(input.input.on_failure.as_str(), "continue" | "stop") {
        return Err(config::problem("invalid_diagnostics_input"));
    }
    Ok(input)
}

struct Outcome {
    state: &'static str,
    code: Option<&'static str>,
    observation: Value,
    selected: Option<SocketAddr>,
}
impl Outcome {
    fn succeeded(observation: Value) -> Self {
        Self {
            state: "succeeded",
            code: None,
            observation,
            selected: None,
        }
    }
    fn failed(code: &'static str, observation: Value) -> Self {
        Self {
            state: "failed",
            code: Some(code),
            observation,
            selected: None,
        }
    }
    fn partial(code: &'static str, observation: Value) -> Self {
        Self {
            state: "partial",
            code: Some(code),
            observation,
            selected: None,
        }
    }
    fn omitted(state: &'static str, code: &'static str) -> Self {
        Self {
            state,
            code: Some(code),
            observation: json!({}),
            selected: None,
        }
    }
}

#[derive(Serialize)]
struct Step {
    id: &'static str,
    operation: &'static str,
    operation_version: u64,
    prerequisites: Vec<&'static str>,
    state: &'static str,
    attempted: bool,
    reason: Option<&'static str>,
    started_offset_ms: Option<u64>,
    duration_ms: Option<u64>,
    timeout_ms: u64,
    observed_at: Option<String>,
    observation: Value,
}

fn steps() -> Vec<Step> {
    IDS.iter()
        .enumerate()
        .map(|(index, id)| Step {
            id,
            operation: OPERATIONS[index],
            operation_version: 1,
            prerequisites: if (1..4).contains(&index) {
                vec![IDS[index - 1]]
            } else {
                vec![]
            },
            state: "blocked",
            attempted: false,
            reason: Some("not_dispatched"),
            started_offset_ms: None,
            duration_ms: None,
            timeout_ms: STEP_LIMIT_MS,
            observed_at: None,
            observation: json!({}),
        })
        .collect()
}

fn system_resolver(bytes: &[u8]) -> Result<Vec<IpAddr>, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "dns_system_configuration_invalid")?;
    let mut nameservers = Vec::new();
    if text.lines().count() > 256 {
        return Err("dns_system_configuration_invalid");
    }
    for line in text.lines() {
        let line = line.split(['#', ';']).next().expect("one split").trim();
        if line.is_empty() {
            continue;
        }
        if !line.bytes().all(|b| b == b'\t' || (32..=126).contains(&b)) {
            return Err("dns_system_configuration_invalid");
        }
        let fields: Vec<_> = line.split_ascii_whitespace().collect();
        match fields[0] {
            "nameserver" if fields.len() == 2 => {
                let address = config::ip(fields[1]).ok_or("dns_system_configuration_invalid")?;
                if address.is_unspecified() || address.is_multicast() || nameservers.len() >= 16 {
                    return Err("dns_system_configuration_invalid");
                }
                nameservers.push(address);
            }
            "search" | "domain"
                if fields.len() >= 2
                    && fields.len() <= 7
                    && fields[1..]
                        .iter()
                        .all(|name| config::hostname(name.trim_end_matches('.'))) => {}
            "options" if fields.len() >= 2 && fields.len() <= 17 => {
                for option in &fields[1..] {
                    if let Some((name, value)) = option.split_once(':') {
                        let value = value
                            .parse::<u64>()
                            .map_err(|_| "dns_system_configuration_invalid")?;
                        if !matches!(
                            (name, value),
                            ("ndots", 0..=15) | ("timeout", 1..=30) | ("attempts", 1..=5)
                        ) {
                            return Err("dns_system_configuration_invalid");
                        }
                    } else if !matches!(
                        *option,
                        "rotate"
                            | "no-check-names"
                            | "inet6"
                            | "edns0"
                            | "single-request"
                            | "single-request-reopen"
                            | "use-vc"
                            | "trust-ad"
                            | "no-reload"
                            | "no-tld-query"
                            | "no-aaaa"
                    ) {
                        return Err("dns_system_configuration_invalid");
                    }
                }
            }
            _ => return Err("dns_system_configuration_invalid"),
        }
    }
    if nameservers.is_empty() {
        return Err("dns_system_configuration_invalid");
    }
    Ok(nameservers)
}

async fn dns(prepared: &Prepared, control: &RunControl, deadline: Instant) -> Outcome {
    let literal = config::ip(&prepared.target.host);
    let mut resolver_identity = Value::Null;
    let mut cname_chains = Value::Null;
    let addresses = if let Some(address) = literal {
        vec![address]
    } else {
        let (address, port, transport, config_sha) = match &prepared.target.resolver {
            Resolver::Explicit {
                address,
                port,
                transport,
            } => (
                config::ip(address).expect("validated endpoint"),
                *port,
                transport.as_str(),
                None,
            ),
            Resolver::System { transport } => {
                let bytes =
                    match config::read_file(Path::new("/etc/resolv.conf"), control, deadline) {
                        Ok(bytes) => bytes,
                        Err(_) => {
                            return Outcome::failed(
                                "dns_system_configuration_unavailable",
                                json!({}),
                            );
                        }
                    };
                let addresses = match system_resolver(&bytes) {
                    Ok(addresses) => addresses,
                    Err(code) => return Outcome::failed(code, json!({})),
                };
                (addresses[0], 53, transport.as_str(), Some(digest(&bytes)))
            }
        };
        resolver_identity = json!({"selection":if config_sha.is_some() {"system_first_nameserver"} else {"explicit"},
            "endpoint":SocketAddr::new(address, port).to_string(),"transport":transport,"configuration_sha256":config_sha,
            "strategy":"a_and_aaaa","attempts_per_query":1,"endpoint_fallback":false,"transport_fallback":false,
            "hosts_file":false,"search_domains":false,"cache":false,"dnssec_validation":false,
            "synthetic_answers":false,"connection_reconnect":false,"max_cname_hops":dns::MAX_CNAME_HOPS,
            "max_queries_per_family":dns::MAX_CNAME_HOPS + 1,"questions":[]});
        let timeout =
            Duration::from_secs(3).min(deadline.saturating_duration_since(Instant::now()));
        let (exchange, _provider) =
            match dns::exchange(SocketAddr::new(address, port), transport, timeout).await {
                Ok(client) => client,
                Err(code) => return Outcome::failed(code, json!({"resolver":resolver_identity})),
            };
        let mut a_questions = Vec::new();
        let mut aaaa_questions = Vec::new();
        let (a, aaaa) = tokio::join!(
            dns::family(
                &exchange,
                &prepared.target.host,
                RecordType::A,
                transport == "udp",
                &mut a_questions
            ),
            dns::family(
                &exchange,
                &prepared.target.host,
                RecordType::AAAA,
                transport == "udp",
                &mut aaaa_questions
            )
        );
        resolver_identity["questions"] = json!(a_questions.iter().chain(&aaaa_questions).map(|query|
            json!({"name":query.name.to_ascii().trim_end_matches('.').to_ascii_lowercase(),"type":query.query_type().to_string()})
        ).collect::<Vec<_>>());
        let (a, aaaa) = match (a, aaaa) {
            (Ok(a), Ok(aaaa)) => (a, aaaa),
            (Err(code), _) | (_, Err(code)) => {
                return Outcome::failed(code, json!({"resolver":resolver_identity}));
            }
        };
        cname_chains = json!({"a":a.chain,"aaaa":aaaa.chain});
        let mut values = a.addresses;
        values.extend(aaaa.addresses);
        if values.len() > 16 {
            return Outcome::failed(
                "dns_answer_limit_exceeded",
                json!({"resolver":resolver_identity,"cname_chains":cname_chains}),
            );
        }
        values
    };
    let mut addresses = addresses;
    addresses.sort();
    addresses.dedup();
    let observation = json!({"hostname":prepared.target.host,"mode":if literal.is_some() {"literal_ip"} else {"dns_lookup"},
        "addresses":addresses.iter().map(IpAddr::to_string).collect::<Vec<_>>(),"resolver":resolver_identity,
        "cname_chains":cname_chains,"selected_peer":Value::Null,"port":prepared.target.port});
    if addresses.is_empty() {
        return Outcome::failed("dns_no_addresses", observation);
    }
    if addresses.iter().any(|address| {
        matches!(address, IpAddr::V6(value) if value.to_ipv4_mapped().is_some())
            || !prepared.allowed.contains(address)
    }) {
        return Outcome::failed("dns_address_denied", observation);
    }
    let peer = SocketAddr::new(addresses[0], prepared.target.port);
    let mut outcome = Outcome::succeeded(observation);
    outcome.observation["selected_peer"] = json!(peer.to_string());
    outcome.selected = Some(peer);
    outcome
}

async fn tcp(prepared: &Prepared, peer: SocketAddr) -> Outcome {
    match TcpStream::connect(peer).await {
        Ok(stream) => {
            if stream.peer_addr().ok() != Some(peer) {
                return Outcome::failed("tcp_peer_mismatch", json!({}));
            }
            Outcome::succeeded(
                json!({"peer":peer.to_string(),"local_address":stream.local_addr().ok().map(|address| address.to_string()),"hostname":prepared.target.host,"port":prepared.target.port}),
            )
        }
        Err(_) => Outcome::failed(
            "tcp_connect_failed",
            json!({"peer":peer.to_string(),"hostname":prepared.target.host,"port":prepared.target.port}),
        ),
    }
}

async fn tls_connect(
    prepared: &Prepared,
    peer: SocketAddr,
) -> Result<(TlsStream<TcpStream>, Value), &'static str> {
    let stream = TcpStream::connect(peer)
        .await
        .map_err(|_| "tcp_connect_failed")?;
    if stream.peer_addr().ok() != Some(peer) {
        return Err("tcp_peer_mismatch");
    }
    let local = stream.local_addr().ok().map(|address| address.to_string());
    let name = rustls::pki_types::ServerName::try_from(prepared.target.host.clone())
        .map_err(|_| "tls_name_invalid")?;
    let stream = TlsConnector::from(prepared.tls.clone())
        .connect(name, stream)
        .await
        .map_err(|_| "tls_validation_failed")?;
    let connection = &stream.get_ref().1;
    let leaf = connection
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .ok_or("tls_certificate_unavailable")?;
    let facts = json!({"peer":peer.to_string(),"local_address":local,"hostname":prepared.target.host,"port":prepared.target.port,
        "certificate_sha256":digest(leaf.as_ref()),"hostname_verified":true,"trust_verified":true,
        "protocol":connection.protocol_version().map(|version| format!("{version:?}")),
        "cipher_suite":connection.negotiated_cipher_suite().map(|suite| format!("{:?}", suite.suite())),
        "alpn":connection.alpn_protocol().map(|protocol| String::from_utf8_lossy(protocol).into_owned())});
    Ok((stream, facts))
}

async fn tls(prepared: &Prepared, peer: SocketAddr) -> Outcome {
    match tls_connect(prepared, peer).await {
        Ok((_stream, facts)) => Outcome::succeeded(facts),
        Err(code) => Outcome::failed(
            code,
            json!({"peer":peer.to_string(),"hostname":prepared.target.host,"port":prepared.target.port}),
        ),
    }
}

type FutureOutcome<'a> = Pin<Box<dyn Future<Output = Outcome> + 'a>>;
struct Active<'a> {
    index: usize,
    started: Instant,
    future: FutureOutcome<'a>,
}

struct Invocation<'a> {
    prepared: &'a Prepared,
    input: &'a PackInput,
    maximum: usize,
    concurrency: u64,
    control: &'a RunControl,
    deadline: Instant,
    start: Instant,
}

fn active<'a>(
    index: usize,
    invocation: &Invocation<'a>,
    selected: Option<SocketAddr>,
    steps: &mut [Step],
) -> Active<'a> {
    let prepared = invocation.prepared;
    let started = Instant::now();
    let remaining = invocation.deadline.saturating_duration_since(started);
    let parent_bound = remaining <= Duration::from_millis(STEP_LIMIT_MS);
    let limit = remaining.min(Duration::from_millis(STEP_LIMIT_MS));
    steps[index].attempted = true;
    steps[index].reason = None;
    steps[index].started_offset_ms =
        Some(started.duration_since(invocation.start).as_millis() as u64);
    steps[index].timeout_ms = limit.as_millis() as u64;
    let operation: FutureOutcome<'a> = match index {
        0 => Box::pin(dns(prepared, invocation.control, started + limit)),
        1 => Box::pin(tcp(prepared, selected.expect("DNS prerequisite"))),
        2 => Box::pin(tls(prepared, selected.expect("DNS prerequisite"))),
        3 => Box::pin(http::observe(
            prepared,
            selected.expect("DNS prerequisite"),
            invocation.input.capture_body,
            invocation.maximum,
        )),
        _ => Box::pin(process::observe(prepared.target.process.as_ref())),
    };
    Active {
        index,
        started,
        future: Box::pin(async move {
            match tokio::time::timeout(limit, operation).await {
                Ok(outcome) => outcome,
                Err(_) => Outcome::omitted(
                    "timed_out",
                    if parent_bound {
                        "parent_deadline_exceeded"
                    } else {
                        "step_deadline_exceeded"
                    },
                ),
            }
        }),
    }
}

async fn await_active(active: &mut Option<Active<'_>>) -> Outcome {
    match active {
        Some(active) => active.future.as_mut().await,
        None => std::future::pending().await,
    }
}

fn completed(active: Active<'_>, outcome: Outcome, steps: &mut [Step]) -> Option<SocketAddr> {
    let step = &mut steps[active.index];
    step.state = outcome.state;
    step.reason = outcome.code;
    step.duration_ms = Some(active.started.elapsed().as_millis() as u64);
    step.observed_at = Some(timestamp());
    step.observation = outcome.observation;
    if matches!(step.state, "unconfigured" | "unsupported") {
        step.attempted = false;
        step.started_offset_ms = None;
        step.duration_ms = None;
        step.observed_at = None;
    }
    outcome.selected
}

async fn execute(invocation: Invocation<'_>, steps: &mut [Step]) -> Result<(), Problem> {
    let mut next = 0;
    let mut local_pending = true;
    let mut selected = None;
    let mut network: Option<Active<'_>> = None;
    let mut local: Option<Active<'_>> = None;
    let mut failed = false;
    let mut parent_expired = false;
    loop {
        let boundary = check(invocation.control, invocation.deadline).and_then(|()| {
            if parent_expired {
                Err(config::problem("deadline_exceeded"))
            } else {
                Ok(())
            }
        });
        if let Err(error) = boundary {
            for active in [&network, &local].into_iter().flatten() {
                let step = &mut steps[active.index];
                step.state = if error.code == "cancelled" {
                    "cancelled"
                } else if error.code == "deadline_exceeded" {
                    "timed_out"
                } else {
                    "failed"
                };
                step.reason = Some(if error.code == "deadline_exceeded" {
                    "parent_deadline_exceeded"
                } else {
                    error.code
                });
                step.duration_ms = Some(active.started.elapsed().as_millis() as u64);
            }
            for step in steps
                .iter_mut()
                .filter(|step| !step.attempted && step.reason == Some("not_dispatched"))
            {
                step.reason = Some(if error.code == "cancelled" {
                    "parent_cancelled"
                } else if error.code == "deadline_exceeded" {
                    "parent_deadline_exceeded"
                } else {
                    "result_consumer_closed"
                });
            }
            return Err(error); // Both active futures and their sockets are dropped on this return.
        }
        let stop = failed && invocation.input.on_failure == "stop";
        if network.is_none() && next < 4 {
            if stop {
                for step in &mut steps[next..4] {
                    step.reason = Some("stopped_after_failure");
                }
                next = 4;
            } else if next > 0 && steps[next - 1].state != "succeeded" {
                for step in &mut steps[next..4] {
                    step.reason = Some("prerequisite_failed");
                }
                next = 4;
            } else if local.is_none() || invocation.concurrency > 1 {
                network = Some(active(next, &invocation, selected, steps));
                next += 1;
            }
        }
        if local_pending && local.is_none() {
            if stop {
                steps[4].reason = Some("stopped_after_failure");
                local_pending = false;
            } else if network.is_none() || invocation.concurrency > 1 {
                local = Some(active(4, &invocation, selected, steps));
                local_pending = false;
            }
        }
        if network.is_none() && local.is_none() {
            break;
        }
        // Control tick is first: a signal/deadline observed before another ready step prohibits
        // further admission, even when a response is simultaneously available.
        tokio::select! {
            biased;
            _ = tokio::time::sleep(Duration::from_millis(10)) => {},
            outcome = await_active(&mut network) => {
                parent_expired |= outcome.code == Some("parent_deadline_exceeded");
                failed |= !matches!(outcome.state, "succeeded" | "unconfigured");
                let active = network.take().expect("active future completed");
                if let Some(peer) = completed(active, outcome, steps) { selected = Some(peer); }
            },
            outcome = await_active(&mut local) => {
                parent_expired |= outcome.code == Some("parent_deadline_exceeded");
                failed |= !matches!(outcome.state, "succeeded" | "unconfigured");
                completed(local.take().expect("active future completed"), outcome, steps);
            },
        }
    }
    check(invocation.control, invocation.deadline)
}

pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    expected_grant: Option<&DiagnosticsGrant>,
    start: Instant,
    result: &mut OperationResult,
) {
    result.effective_limits.timeout_ms = request.limits.timeout_ms.min(60_000);
    result.effective_limits.max_output_bytes = request.limits.max_output_bytes.min(16 * 1024);
    result.effective_limits.concurrency = Some(request.limits.concurrency.unwrap_or(2));
    result.coverage.scope = "Only the configured DNS/TCP/TLS/HTTP and exact local process observations; no service health assessment.".into();
    result.coverage.state = "not_established".into();
    let deadline = start + Duration::from_millis(result.effective_limits.timeout_ms);
    let mut observations = steps();
    let outcome = (|| {
        if !cfg!(target_os = "linux") {
            return Err(Problem::unsupported(
                "unsupported_platform",
                "Connectivity diagnostics are currently verified only on Linux.",
            ));
        }
        check(control, deadline)?;
        let input = input(&request.inputs)?;
        if expected_grant.is_some_and(|grant| !grant.targets.contains_key(&request.target.id)) {
            return Err(config::problem("diagnostics_target_not_granted"));
        }
        let prepared = config::load(
            context.diagnostics_config_path.as_deref(),
            &request.target.id,
            control,
            deadline,
        )?;
        if let Some(grant) = expected_grant {
            grant.validate(&request.target.id, &prepared.identity)?;
        }
        check(control, deadline)?;
        let definition = metadata();
        result.resolved_target = json!({"connection":request.target.id,"profile":"operator-configured","hostname":prepared.target.host,"port":prepared.target.port,"effects":"observe"});
        result.sources.push(json!({"reference":format!("diagnostics-config:{}",prepared.identity["configuration_sha256"].as_str().expect("config identity")),"observed_at":timestamp(),"evidence_state":"sourced","untrusted":true}));
        result.sources.push(json!({"reference":format!("diagnostics-pack:{}",definition["sha256"].as_str().expect("pack identity")),"observed_at":timestamp(),"evidence_state":"sourced","untrusted":false}));
        result.data = json!({"pack":definition,"binding":{"target":request.target.id,"hostname":prepared.target.host,"port":prepared.target.port,
            "path":prepared.target.path,"allowed_ips":prepared.target.allowed_ips,"identity":prepared.identity},
            "vantage":process::vantage(),"input":{"capture_body":input.input.capture_body,"on_failure":input.input.on_failure},
            "steps":[],"cancellation_signal":Value::Null,"redaction":{"applied":false,"sensitive_content_detection":false}});
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| config::problem("diagnostics_runtime_unavailable"))?;
        result.effect_outcome = "not_applicable".into();
        let execution = runtime.block_on(execute(
            Invocation {
                prepared: &prepared,
                input: &input.input,
                maximum: result.effective_limits.max_output_bytes,
                concurrency: result
                    .effective_limits
                    .concurrency
                    .expect("effective concurrency"),
                control,
                deadline,
                start,
            },
            &mut observations,
        ));
        // No spawned blocking task exists. Drop all check/resolver futures, then shut down the
        // invocation-owned reactor, cancelling any Hickory background DNS connection workers.
        runtime.shutdown_timeout(Duration::from_millis(100));
        execution?;
        let successes = observations
            .iter()
            .filter(|step| step.state == "succeeded")
            .count();
        result.execution.status = if successes == 5 {
            Status::Succeeded
        } else if successes > 0 || observations.iter().any(|step| step.state == "partial") {
            Status::Partial
        } else {
            Status::Failed
        };
        result.coverage.state = if successes == 5 {
            "complete"
        } else if observations.iter().any(|step| {
            !step
                .observation
                .as_object()
                .is_some_and(serde_json::Map::is_empty)
        }) {
            "partial"
        } else {
            "not_established"
        }
        .into();
        for step in &observations {
            if let Some(code) = step.reason {
                result.error(code, "A diagnostics check could not complete; see its typed state and prerequisite reason. Private transport details are withheld.");
            }
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        if matches!(
            error.code,
            "cancelled" | "deadline_exceeded" | "result_consumer_closed"
        ) {
            result.execution.status = if error.code == "cancelled" {
                Status::Cancelled
            } else if error.code == "deadline_exceeded" {
                Status::TimedOut
            } else {
                Status::Failed
            };
            result.error(error.code, "The diagnostics invocation stopped at its cancellation, parent deadline or result-consumer boundary.");
            result.coverage.state = if observations.iter().any(|step| step.state == "succeeded") {
                "partial"
            } else {
                "not_established"
            }
            .into();
        } else {
            result.reject(error);
        }
        for step in &mut observations {
            if step.reason == Some("not_dispatched") {
                step.reason = Some("admission_refused");
            }
        }
    }
    if result.data.as_object().is_some_and(|data| !data.is_empty()) {
        result.data["steps"] =
            serde_json::to_value(observations).expect("bounded typed observations");
        let signal = control.signal.load(Ordering::Relaxed);
        if signal != 0 {
            result.data["cancellation_signal"] = json!(signal);
        }
        if serde_json::to_vec(&result.data)
            .expect("typed diagnostics data")
            .len()
            > 64 * 1024
        {
            result.data = json!({"observations_omitted":true,"reason":"diagnostics_data_too_large","cancellation_signal":if signal == 0 {None} else {Some(signal)}});
            if result.execution.status != Status::Cancelled
                && result.execution.status != Status::TimedOut
            {
                result.execution.status = Status::Failed;
            }
            result.coverage.state = "not_established".into();
            result.error(
                "diagnostics_data_too_large",
                "Required observations were omitted to enforce the 64 KiB structured-data ceiling.",
            );
        }
    }
    result.coverage.limitations = vec![
        "These are observations from the recorded Linux network/PID vantage, not a service-health, systemd, Windows or live-compatibility assessment.".into(),
        "Configured destinations and public identity remain operator-owned. No protected identity, persistence, redirect, proxy, retry or automatic endpoint fallback is provided.".into(),
        "Captured response content is untrusted and is not automatically redacted or decompressed. Only the fixed header allowlist is returned.".into(),
    ];
}
