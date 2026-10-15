//! Offline comparison of verified retained observations.
use super::{
    frames::{self, Normalization, Window},
    store::{Store, StoreError, VerifiedBundle},
};
use crate::{
    OperatorContext, RunControl, queries,
    request::{Problem, Request, parse_bounded_json},
    result::{MAX_RECEIPT_DEPTH, OperationResult, Status},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub(super) fn active(control: &RunControl, deadline: Instant) -> Result<(), &'static str> {
    if control.signal.load(Ordering::Relaxed) != 0 {
        return Err("cancelled");
    }
    if Instant::now() >= deadline {
        return Err("deadline_exceeded");
    }
    #[cfg(target_os = "linux")]
    if let Some(fd) = control.result_fd {
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: the initialized descriptor record is borrowed for a nonblocking poll.
        if unsafe { libc::poll(&mut poll, 1, 0) } > 0
            && poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
        {
            return Err("result_consumer_closed");
        }
    }
    Ok(())
}
fn stop(result: &mut OperationResult, code: &'static str, control: &RunControl) {
    result.execution.status = if code == "deadline_exceeded" {
        Status::TimedOut
    } else {
        Status::Cancelled
    };
    if result.data.is_object() {
        result.data["state"] = json!("partial");
        result.data["cancellation_signal"] = json!(control.signal.load(Ordering::Relaxed));
    }
    result.coverage.state = "partial".into();
    result.error(
        code,
        "Comparison stopped before all retained observations were analyzed.",
    );
}
fn store_failure(result: &mut OperationResult, error: StoreError, control: &RunControl) {
    super::fail(result, error);
    let signal = control.signal.load(Ordering::Relaxed);
    if signal != 0 {
        if !result.data.is_object() {
            result.data = json!({});
        }
        result.data["cancellation_signal"] = json!(signal);
    }
}
fn hidden(value: &Value) -> bool {
    match value {
        Value::String(s) => s.contains("[REDACTED]"),
        Value::Array(a) => a.iter().any(hidden),
        Value::Object(o) => {
            o.get("redacted") == Some(&Value::Bool(true))
                || o.iter().any(|(k, v)| k.contains("[REDACTED]") || hidden(v))
        }
        _ => false,
    }
}
pub(super) fn window(payload: &Value) -> Result<Window, &'static str> {
    let parse = |key: &str| {
        OffsetDateTime::parse(
            payload["window"][key].as_str().ok_or("invalid_window")?,
            &Rfc3339,
        )
        .map(|v| v.unix_timestamp_nanos())
        .map_err(|_| "invalid_window")
    };
    let from_ns = parse("from")?;
    let to_ns = parse("to")?;
    if from_ns >= to_ns {
        return Err("invalid_window");
    }
    Ok(Window { from_ns, to_ns })
}
pub(crate) fn validate_input(value: &Value) -> Result<(), Problem> {
    input(value).map(|_| ())
}

fn input(value: &Value) -> Result<Normalization, Problem> {
    let bad = || {
        Problem::invalid(
            "invalid_comparison_input",
            "Supply exactly before and after bundle references and an explicit supported normalization.",
        )
    };
    let object = value.as_object().ok_or_else(bad)?;
    if object.len() != 3
        || object
            .keys()
            .any(|k| !matches!(k.as_str(), "before" | "after" | "normalization"))
    {
        return Err(bad());
    }
    super::reference(&value["before"])?;
    super::reference(&value["after"])?;
    match value["normalization"].as_str() {
        Some("exact-window-v1") => Ok(Normalization::ExactWindow),
        Some("window-offset-v1") => Ok(Normalization::WindowOffset),
        _ => Err(bad()),
    }
}
fn source(bundle: &VerifiedBundle) -> Value {
    let p = &bundle.manifest.payload;
    json!({"bundle":bundle.reference,"window":p["window"],"capture":p["capture"],"manifest_state":bundle.manifest.state,"started_at":p["started_at"],"finalized_at":p["finalized_at"]})
}
pub(super) fn identities(before: &Value, after: &Value) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    for p in [before, after] {
        if p["producer"]["id"] != "save" || p["producer"]["spec_version"] != "0.1" {
            reasons.push("unsupported_producer");
        }
        if hidden(&p["selection"]) || hidden(&p["connection"]) {
            reasons.push("redacted_identity");
        }
    }
    if before["selection"] != after["selection"] {
        reasons.push("incompatible_selection");
    }
    for field in ["origin", "organization_id"] {
        if before["connection"][field] != after["connection"][field] {
            reasons.push("incompatible_connection");
        }
    }
    if before["pack"] != after["pack"] {
        reasons.push("incompatible_pack");
    }
    reasons.sort_unstable();
    reasons.dedup();
    reasons
}
fn revisions(before: &Value, after: &Value) -> Vec<&'static str> {
    [
        "/producer/version",
        "/connection/id",
        "/provenance/grafana_config_sha256",
        "/provenance/grafana_ca",
        "/provenance/context/source/id",
        "/provenance/context/source/revision",
        "/provenance/context/source/sha256",
        "/provenance/context/source/observed_at",
        "/provenance/context/record",
    ]
    .into_iter()
    .filter(|path| before.pointer(path) != after.pointer(path))
    .collect()
}
pub(super) struct Observation {
    pub data: Value,
    pub partial: bool,
    pub interval: u64,
}
pub(super) fn observation(
    bundle: &VerifiedBundle,
    id: &str,
    kind: &str,
) -> Result<Observation, &'static str> {
    let p = &bundle.manifest.payload;
    let step = p["steps"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["id"] == id))
        .ok_or("missing_check")?;
    if step["artifact"].is_null() {
        return Err("missing_check");
    }
    let bytes = bundle.artifacts.get(id).ok_or("missing_check")?;
    let receipt = parse_bounded_json(bytes, MAX_RECEIPT_DEPTH).map_err(|_| "invalid_receipt")?;
    if receipt["operation"] != "query.run"
        || receipt["operation_version"] != 1
        || receipt["spec_version"] != "0.1"
    {
        return Err("unsupported_receipt");
    }
    if receipt["execution"]["status"] != "succeeded" {
        return Err("unusable_receipt");
    }
    let data = &receipt["data"];
    if data["adapter"]["id"] != "grafana"
        || data["adapter"]["version"] != 1
        || data["adapter"]["api_profile"] != "grafana-legacy-v1"
    {
        return Err("unsupported_adapter");
    }
    for key in ["definition", "parameters", "window", "request", "binding"] {
        if hidden(&data[key]) {
            return Err("redacted_identity");
        }
    }
    for key in ["origin", "organization_id"] {
        if data["binding"][key] != p["connection"][key] {
            return Err("incompatible_binding");
        }
    }
    if data["binding"]["organization_verified"] != true {
        return Err("unverified_organization");
    }
    let expected_id = if kind == "prometheus" {
        "prometheus.up-by-job"
    } else {
        "loki.logs-by-job"
    };
    let compiled=queries::compile(&json!({"id":expected_id,"version":1,"datasource":data["request"]["datasource"],"parameters":data["parameters"],"window":{"kind":"absolute","from":p["window"]["from"],"to":p["window"]["to"]}}),p["started_at"].as_str().ok_or("invalid_window")?).map_err(|_|"invalid_saved_query")?;
    if data["definition"] != compiled.definition
        || step["definition"] != compiled.definition
        || data["window"] != p["window"]
    {
        return Err("incompatible_query");
    }
    for key in ["datasource", "kind", "from", "to", "expr"] {
        if data["request"][key] != compiled.input[key] {
            return Err("invalid_compiled_request");
        }
    }
    if data["request"]["datasource"]
        .as_str()
        .is_none_or(str::is_empty)
    {
        return Err("invalid_datasource");
    }
    let w = window(p)?;
    let duration_ms = (w.to_ns - w.from_ns) / 1_000_000;
    let interval =
        u64::try_from(1000.max((duration_ms + 999) / 1000)).map_err(|_| "invalid_window")?;
    if data["request"]["interval_ms"] != interval
        || data["request"]["max_data_points"] != 1000
        || data["request"]["max_lines"]
            != if kind == "loki" {
                json!(500)
            } else {
                Value::Null
            }
    {
        return Err("invalid_sampling");
    }
    let results = data["response"]["results"]
        .as_object()
        .ok_or("missing_response")?;
    if results.len() != 1 || !results.contains_key("A") {
        return Err("unsupported_response");
    }
    let response = &results["A"];
    if data["response"]
        .get("error")
        .is_some_and(|e| !e.is_null() && e != "")
        || !response.is_object()
        || response
            .get("error")
            .is_some_and(|e| !e.is_null() && e != "")
        || response.get("status").is_some_and(|s| {
            let status = s.as_u64().or_else(|| {
                s.as_str()
                    .filter(|s| s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|s| s.parse::<u64>().ok())
            });
            status.is_none_or(|s| !(100..400).contains(&s))
        })
    {
        return Err("unusable_response");
    }
    let partial = receipt["coverage"]["state"] != "not_established"
        || receipt["output"]["stdout_truncated"] == true
        || receipt["output"]["stderr_truncated"] == true
        || receipt["errors"].as_array().is_none_or(|v| !v.is_empty());
    Ok(Observation {
        data: data.clone(),
        partial,
        interval,
    })
}

pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    let normalization = match input(&request.inputs) {
        Ok(v) => v,
        Err(p) => {
            result.reject(p);
            return;
        }
    };
    let Some(options) = &context.evidence_store else {
        result.reject(Problem::invalid(
            "evidence_store_required",
            "Attach an explicit private --state-dir before comparing evidence.",
        ));
        return;
    };
    if !cfg!(target_os = "linux") {
        result.reject(Problem::unsupported(
            "unsupported_platform",
            "Safe local evidence storage is implemented on Linux only.",
        ));
        return;
    }
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    let check = || active(control, deadline);
    if let Err(code) = check() {
        stop(result, code, control);
        return;
    }
    let store = match Store::open_validated(
        options,
        false,
        deadline,
        super::validate_bundle,
        Some(Arc::clone(&control.signal)),
    ) {
        Ok(s) => s,
        Err(e) => {
            store_failure(result, e, control);
            return;
        }
    };
    result.effect_outcome = "not_applicable".into();
    let mut bundles = Vec::new();
    for side in ["before", "after"] {
        if let Err(code) = check() {
            stop(result, code, control);
            return;
        }
        let (id, pin) = super::reference(&request.inputs[side]).expect("validated reference");
        let bundle = match store.load_verified(id) {
            Ok(b) => b,
            Err(e) => {
                store_failure(result, e, control);
                return;
            }
        };
        if pin.is_some_and(|pin| pin != bundle.reference.digest) {
            super::fail(result, StoreError::new("manifest_digest_mismatch", false));
            return;
        }
        bundles.push(bundle);
    }
    analyze_bundles(
        request,
        control,
        deadline,
        result,
        &bundles[0],
        &bundles[1],
        normalization,
    );
}

pub(super) fn analyze_bundles(
    request: &Request,
    control: &RunControl,
    deadline: Instant,
    result: &mut OperationResult,
    before: &VerifiedBundle,
    after: &VerifiedBundle,
    normalization: Normalization,
) {
    let mut check = || active(control, deadline);
    let bp = &before.manifest.payload;
    let ap = &after.manifest.payload;
    result.sources = [before, after].into_iter().map(|bundle| {
        // Integrity checks do not prove the source observations' truth or authorship.
        json!({
            "reference":format!("evidence-bundle:{}@{}",bundle.reference.id,bundle.reference.digest),
            "observed_at":bundle.manifest.payload["started_at"].as_str(),
            "evidence_state":"sourced",
            "untrusted":true
        })
    }).collect();
    let mut reasons = identities(bp, ap);
    let windows = match (window(bp), window(ap)) {
        (Ok(b), Ok(a)) => {
            if match normalization {
                Normalization::ExactWindow => b.from_ns != a.from_ns || b.to_ns != a.to_ns,
                Normalization::WindowOffset => b.to_ns - b.from_ns != a.to_ns - a.from_ns,
            } {
                reasons.push("incompatible_windows");
            }
            Some((b, a))
        }
        _ => {
            reasons.push("invalid_window");
            None
        }
    };
    let compatible = reasons.is_empty();
    result.data = json!({"format_version":1,"algorithm":"returned-observations-v1","normalization":request.inputs["normalization"],"scope":"returned_observations","state":"partial","before":source(before),"after":source(after),"compatibility":{"state":if compatible {"compatible"} else {"incompatible"},"reasons":reasons},"provenance_changes":revisions(bp,ap),"checks":[]});
    result.coverage.scope = "Returned observations in the two verified local bundles only.".into();
    result.coverage.limitations.push("Source coverage remains unestablished: comparison does not prove full telemetry, service health, log rates, causation or remediation.".into());
    let mut partial = before.manifest.state != "sealed"
        || after.manifest.state != "sealed"
        || bp["capture"]["state"] != "complete"
        || ap["capture"]["state"] != "complete";
    let mut source_limitations = Vec::new();
    if before.manifest.state != "sealed" || after.manifest.state != "sealed" {
        source_limitations.push("interrupted_source_bundle");
    }
    if bp["capture"]["state"] != "complete" || ap["capture"]["state"] != "complete" {
        source_limitations.push("partial_source_capture");
    }
    result.data["source_limitations"] = json!(source_limitations);
    let mut count = request.limits.max_items.unwrap_or(100).min(1000) as usize;
    let mut bytes = request.limits.max_output_bytes.min(1024 * 1024);
    result.effective_limits.max_items = Some(count as u64);
    result.effective_limits.max_output_bytes = bytes;
    for (index, (id, kind, unit)) in [
        ("metrics", "prometheus", "dimensionless"),
        ("logs", "loki", "entries"),
    ]
    .into_iter()
    .enumerate()
    {
        if let Err(code) = check() {
            stop(result, code, control);
            return;
        }
        let definition=queries::list()["queries"].as_array().expect("installed queries").iter().find(|q|q["id"]==if kind=="prometheus" {"prometheus.up-by-job"} else {"loki.logs-by-job"}).map(|q|json!({"id":q["id"],"version":q["version"],"dialect":q["dialect"],"sha256":q["definition_sha256"]})).expect("installed query");
        let mut output = json!({"id":id,"kind":kind,"state":"incompatible","reasons":[],"definition":definition,"unit":unit,"summary":{},"details":[],"omitted_details":0});
        if !compatible {
            output["reasons"] = json!(reasons);
        } else {
            let comparison = (|| {
                let b = observation(before, id, kind)?;
                check()?;
                let a = observation(after, id, kind)?;
                for key in [
                    "datasource",
                    "kind",
                    "interval_ms",
                    "max_data_points",
                    "max_lines",
                ] {
                    if b.data["request"][key] != a.data["request"][key] {
                        return Err("incompatible_sampling_or_datasource");
                    }
                }
                if b.data["parameters"] != a.data["parameters"]
                    || b.data["definition"] != a.data["definition"]
                {
                    return Err("incompatible_query");
                }
                let (bw, aw) = windows.expect("compatible windows");
                let mut comparison = frames::compare(
                    kind,
                    &b.data["response"]["results"]["A"],
                    &a.data["response"]["results"]["A"],
                    bw,
                    aw,
                    normalization,
                    b.interval,
                    count,
                    // Every later check emits at least its empty array. Reserve
                    // those delimiters even if that check cannot be compared.
                    bytes.saturating_sub((1 - index) * 2),
                    &mut check,
                )?;
                if b.partial || a.partial {
                    comparison.partial = true;
                    comparison.reasons.push("partial_source_receipt");
                }
                Ok(comparison)
            })();
            match comparison {
                Ok(c) => {
                    // Once a detail is omitted, later checks may still compute
                    // counts but cannot restart the global emitted prefix.
                    if c.omitted_details != 0 {
                        count = 0;
                    }
                    output["state"] = json!(if c.partial { "partial" } else { "complete" });
                    output["reasons"] = json!(c.reasons);
                    output["summary"] = c.summary;
                    output["details"] = json!(c.details);
                    output["omitted_details"] = json!(c.omitted_details);
                    partial |= c.partial;
                }
                Err(code) => {
                    if matches!(
                        code,
                        "cancelled" | "deadline_exceeded" | "result_consumer_closed"
                    ) {
                        stop(result, code, control);
                        return;
                    }
                    output["state"] = json!(if matches!(
                        code,
                        "missing_check"
                            | "unusable_receipt"
                            | "missing_response"
                            | "unusable_response"
                    ) {
                        "missing"
                    } else {
                        "incompatible"
                    });
                    output["reasons"] = json!([code]);
                    partial = true;
                }
            }
        }
        count = count.saturating_sub(output["details"].as_array().expect("details array").len());
        bytes = bytes.saturating_sub(
            serde_json::to_vec(&output["details"])
                .expect("serializable details")
                .len(),
        );
        result.data["checks"]
            .as_array_mut()
            .expect("checks array")
            .push(output);
    }
    if let Err(code) = check() {
        stop(result, code, control);
        return;
    }
    result.data["state"] = json!(if !compatible {
        "incompatible"
    } else if partial {
        "partial"
    } else {
        "complete"
    });
    result.execution.status = if compatible && !partial {
        Status::Succeeded
    } else {
        Status::Partial
    };
    result.coverage.state = if compatible && !partial {
        "not_established"
    } else {
        "partial"
    }
    .into();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_input_refuses_null_unknown_fields_and_implicit_policy() {
        for value in [
            Value::Null,
            json!({}),
            json!({"before":null,"after":null,"normalization":"exact-window-v1"}),
            json!({"before":{},"after":{},"normalization":"automatic"}),
        ] {
            assert!(input(&value).is_err());
        }
    }
    #[test]
    fn window_preserves_nanoseconds_and_equivalent_offsets() {
        let a=window(&json!({"window":{"from":"2026-10-10T00:00:00.000000001Z","to":"2026-10-10T00:00:01Z"}})).unwrap();
        let b=window(&json!({"window":{"from":"2026-10-09T19:00:00.000000001-05:00","to":"2026-10-09T19:00:01-05:00"}})).unwrap();
        assert_eq!(a.from_ns, b.from_ns);
        assert_eq!(a.to_ns - a.from_ns, 999_999_999);
    }
    #[test]
    fn redacted_nested_identity_is_never_comparable() {
        assert!(hidden(&json!({"job":{"redacted":true}})));
        assert!(hidden(&json!({"x":"pre[REDACTED]post"})));
        assert!(!hidden(&json!({"job":"public"})));
    }
}
