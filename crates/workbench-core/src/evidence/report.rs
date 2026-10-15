//! Evidence-bound reports. Source integrity is verified; source truth is never inferred.
use super::{
    compare,
    frames::Normalization,
    offline,
    store::{EvidenceStoreOptions, Store, StoreError, VerifiedBundle},
};
use crate::{
    OperatorContext, RunControl,
    request::{MAX_RESULT_BYTES, MAX_STREAM_BYTES, Problem, Request, bounded},
    result::{OperationResult, Status, timestamp},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

const MAX_REPORT_BYTES: usize = 512 * 1024;
const DETAIL_BYTES: usize = 32 * 1024;
const DETAIL_ITEMS: u64 = 32;
const HISTORICAL: &str = "Historical observations are not current service health. Missing samples are not zero; absent logs are not an all-clear. Source integrity does not establish truth, authorship, causation or complete telemetry coverage.";
const OMITTED: &str = "The complete rendered document exceeded its output budget and was omitted; the structured report remains available.";

struct Inputs {
    bundles: Vec<Value>,
    format: String,
    title: String,
    proposals: Vec<Value>,
}

pub(crate) fn normalize_input(value: &Value) -> Result<Value, Problem> {
    let input = input(value)?;
    Ok(
        json!({"bundles":input.bundles,"format":input.format,"title":input.title,"proposals":input.proposals}),
    )
}

fn input(value: &Value) -> Result<Inputs, Problem> {
    let bad = || {
        Problem::invalid(
            "invalid_report_input",
            "Supply one to four distinct generated bundle references, markdown or json format, an optional bounded title and up to eight evidence-bound pending proposals; unknown fields and paths are not accepted.",
        )
    };
    let object = value.as_object().ok_or_else(bad)?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "bundles" | "format" | "title" | "proposals"))
    {
        return Err(bad());
    }
    let bundles = value["bundles"]
        .as_array()
        .filter(|v| (1..=4).contains(&v.len()))
        .ok_or_else(bad)?;
    let mut ids = BTreeSet::new();
    for reference in bundles {
        let (id, _) = super::reference(reference).map_err(|_| bad())?;
        if !ids.insert(id) {
            return Err(bad());
        }
    }
    let format = value["format"]
        .as_str()
        .filter(|v| matches!(*v, "markdown" | "json"))
        .ok_or_else(bad)?;
    let title = match object.get("title") {
        None => "Evidence handoff",
        Some(value) => value
            .as_str()
            .filter(|s| bounded(s, 1, 160))
            .ok_or_else(bad)?,
    };
    let proposals = match object.get("proposals") {
        None => vec![],
        Some(value) => value
            .as_array()
            .filter(|v| v.len() <= 8)
            .ok_or_else(bad)?
            .clone(),
    };
    for proposal in &proposals {
        let fields = proposal.as_object().ok_or_else(bad)?;
        if fields.len() != 4
            || fields
                .keys()
                .any(|key| !matches!(key.as_str(), "kind" | "title" | "content" | "evidence"))
            || !matches!(
                proposal["kind"].as_str(),
                Some("service_card" | "alert_card" | "runbook")
            )
            || !proposal["title"]
                .as_str()
                .is_some_and(|s| bounded(s, 1, 160))
            || !proposal["content"]
                .as_str()
                .is_some_and(|s| bounded(s, 1, 2000))
        {
            return Err(bad());
        }
        let references = proposal["evidence"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= bundles.len())
            .ok_or_else(bad)?;
        let mut unique = BTreeSet::new();
        for reference in references {
            let id = reference.as_str().ok_or_else(bad)?;
            if !ids.contains(id) || !unique.insert(id) {
                return Err(bad());
            }
        }
    }
    Ok(Inputs {
        bundles: bundles.clone(),
        format: format.into(),
        title: title.into(),
        proposals,
    })
}

fn active(control: &RunControl, deadline: Instant) -> Result<(), StoreError> {
    compare::active(control, deadline).map_err(|code| {
        StoreError::new(
            match code {
                "deadline_exceeded" => "store_deadline_exceeded",
                "cancelled" => "store_cancelled",
                _ => "result_consumer_closed",
            },
            false,
        )
    })
}

fn failure(result: &mut OperationResult, error: StoreError, control: &RunControl) {
    let closed = error.code == "result_consumer_closed";
    result.data = json!({});
    result.sources.clear();
    super::fail(result, error);
    if closed {
        result.execution.status = Status::Cancelled;
    }
    let signal = control.signal.load(Ordering::Relaxed);
    if signal != 0 {
        result.data["cancellation_signal"] = json!(signal);
    }
}

pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    let inputs = match input(&request.inputs) {
        Ok(inputs) => inputs,
        Err(problem) => {
            result.reject(problem);
            return;
        }
    };
    let Some(options) = &context.evidence_store else {
        result.reject(Problem::invalid(
            "evidence_store_required",
            "Attach an explicit private --state-dir before creating an evidence report.",
        ));
        return;
    };
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    let worker = std::thread::scope(|scope| {
        std::thread::Builder::new().name("evidence-report".into()).spawn_scoped(scope, || {
            if let Err(code) = offline::profile::install() {
                result.reject(Problem::unsupported(code,"Report creation requires the Linux x86_64 offline execution profile; no evidence was opened."));return;
            }
            match build(&inputs, options, control, deadline) {
                Ok(report) => {
                    match active(control, deadline) {
                        Ok(()) => publish(report, &inputs.format, request.limits.max_output_bytes, result),
                        Err(error) => { failure(result, error, control); return; }
                    }
                    if let Err(error) = active(control, deadline) {
                        // Never return a completed rendering after a cancelled/expired
                        // serialization pass. Original stored evidence is unchanged.
                        failure(result, error, control);
                    }
                },
                Err(error) => failure(result,error,control),
            }
        }).map(|worker|worker.join())
    });
    if !matches!(worker, Ok(Ok(()))) {
        result.reject(Problem::unsupported(
            "offline_worker_failed",
            "The bounded offline report worker could not complete.",
        ));
    }
}

fn build(
    inputs: &Inputs,
    options: &EvidenceStoreOptions,
    control: &RunControl,
    deadline: Instant,
) -> Result<Value, StoreError> {
    active(control, deadline)?;
    let store = Store::open_validated(
        options,
        false,
        deadline,
        super::validate_bundle,
        Some(Arc::clone(&control.signal)),
    )?;
    let mut bundles = Vec::new();
    for reference in &inputs.bundles {
        active(control, deadline)?;
        let (id, pin) = super::reference(reference).expect("validated report reference");
        let bundle = store.load_verified(id)?;
        if pin.is_some_and(|pin| pin != bundle.reference.digest) {
            return Err(StoreError::new("manifest_digest_mismatch", false));
        }
        bundles.push(bundle);
    }
    // Retain the verified bytes and store lock through analysis. No second lookup,
    // unpinned re-read or operator configuration/provider is needed for comparisons.
    let mut observations = Vec::new();
    let mut sources = Vec::new();
    let mut partial = false;
    for bundle in &bundles {
        active(control, deadline)?;
        let request = child(
            "evidence.replay",
            json!({"id":bundle.reference.id,"expected_manifest_sha256":bundle.reference.digest}),
        );
        let mut receipt = OperationResult::new(&request);
        offline::analyze_bundle(&request, control, deadline, &mut receipt, bundle);
        active(control, deadline)?;
        if !matches!(
            receipt.execution.status,
            Status::Succeeded | Status::Partial
        ) {
            return Err(StoreError::new("report_analysis_failed", false));
        }
        partial |= receipt.data["state"] != "complete";
        let mut observation = receipt.data["observation"].clone();
        observation["bundle"] = json!(bundle.reference);
        observation["state"] = receipt.data["state"].clone();
        observation["source_limitations"] = receipt.data["source_limitations"].clone();
        observation["checks"] = receipt.data["checks"].clone();
        observation["provenance"] = bundle.manifest.payload["provenance"].clone();
        observation["redaction"] = bundle.manifest.payload["redaction"].clone();
        observations.push(observation);
        sources.push(source(bundle));
    }
    let mut comparisons = Vec::new();
    for before in 0..bundles.len() {
        for after in before + 1..bundles.len() {
            active(control, deadline)?;
            let request = child(
                "evidence.compare",
                json!({"before":{"id":bundles[before].reference.id,"expected_manifest_sha256":bundles[before].reference.digest},"after":{"id":bundles[after].reference.id,"expected_manifest_sha256":bundles[after].reference.digest},"normalization":"exact-window-v1"}),
            );
            let mut receipt = OperationResult::new(&request);
            compare::analyze_bundles(
                &request,
                control,
                deadline,
                &mut receipt,
                &bundles[before],
                &bundles[after],
                Normalization::ExactWindow,
            );
            active(control, deadline)?;
            if !matches!(
                receipt.execution.status,
                Status::Succeeded | Status::Partial
            ) {
                return Err(StoreError::new("report_comparison_failed", false));
            }
            partial |= receipt.data["state"] != "complete";
            comparisons.push(json!({"before":bundles[before].reference,"after":bundles[after].reference,
                "state":receipt.data["state"],"finding":finding(&receipt.data),
                "compatibility":receipt.data["compatibility"],"provenance_changes":receipt.data["provenance_changes"],
                "source_limitations":receipt.data["source_limitations"],"checks":receipt.data["checks"]}));
        }
    }
    let proposals:Vec<_>=inputs.proposals.iter().map(|proposal| {
        let evidence:Vec<_>=proposal["evidence"].as_array().expect("validated proposal").iter().map(|id| {
            let bundle=bundles.iter().find(|bundle| id.as_str() == Some(bundle.reference.id.as_str())).expect("selected proposal reference");
            json!(bundle.reference)
        }).collect();
        json!({"kind":proposal["kind"],"title":proposal["title"],"content":proposal["content"],"evidence":evidence,
            "provenance":"operator_proposed","review_status":"pending"})
    }).collect();
    let mut limitations=vec![HISTORICAL.to_owned(),"Only installed saved-query frame profiles are analyzed. Details are bounded; omission counts, source redactions and incompatible checks remain explicit.".into()];
    if partial {
        limitations.push("Some historical observations or comparisons are missing, incompatible, partial or detail-limited. Review each check before drawing conclusions.".into());
    }
    if !proposals.is_empty() {
        limitations.push("Knowledge text is supplied by the operator, is not a verified observation and remains pending human review. Nothing was applied or published.".into());
    }
    let mut questions = vec![
        "Which separately requested live observations are needed to establish current conditions?"
            .to_owned(),
    ];
    if partial {
        questions.push("Which missing, redacted, incompatible or omitted observations need operator review or a new collection?".into());
    }
    if comparisons
        .iter()
        .any(|comparison| comparison["finding"] == "disagreement")
    {
        questions.push("What explains the differing retained observations? Check sampling, source identity and collection timing before proposing a cause.".into());
    }
    if !proposals.is_empty() {
        questions.push(
            "Should a human accept, revise or reject each evidence-bound knowledge proposal?"
                .into(),
        );
    }
    active(control, deadline)?;
    let report = json!({"title":inputs.title,"generated_at":timestamp(),"tool":{"id":"save","version":env!("CARGO_PKG_VERSION")},
        "state":if partial {"partial"} else {"complete"},"scope":"historical_evidence","profile":"linux-offline-v1",
        "sources":sources,"observations":observations,"comparisons":comparisons,"limitations":limitations,
        "next_questions":questions,"knowledge_proposals":proposals});
    if serde_json::to_vec(&report)
        .expect("serializable report")
        .len()
        // Reserve metadata space for whole-document omission and final receipt guards.
        > MAX_REPORT_BYTES - 1024
    {
        return Err(StoreError::new("report_structure_too_large", false));
    }
    Ok(report)
}

fn child(operation: &str, inputs: Value) -> Request {
    let mut request = Request::new(operation, inputs);
    request.limits.max_items = Some(DETAIL_ITEMS);
    request.limits.max_output_bytes = DETAIL_BYTES;
    request
}

fn source(bundle: &VerifiedBundle) -> Value {
    json!({"bundle":bundle.reference,"reference":format!("evidence-bundle:{}@{}",bundle.reference.id,bundle.reference.digest),
        "observed_at":bundle.manifest.payload["started_at"],"evidence_state":"sourced","untrusted":true})
}

fn finding(comparison: &Value) -> &'static str {
    let checks = comparison["checks"].as_array().expect("comparison checks");
    let changed = checks.iter().any(|check| {
        [
            "series_added",
            "series_removed",
            "samples_added",
            "samples_removed",
            "samples_changed",
            "entries_added",
            "entries_removed",
        ]
        .iter()
        .any(|key| {
            check["summary"][*key]
                .as_u64()
                .is_some_and(|count| count > 0)
        })
    });
    if changed {
        "disagreement"
    } else if comparison["compatibility"]["state"] == "incompatible"
        || checks.iter().any(|check| check["state"] == "incompatible")
    {
        "incomparable"
    } else if comparison["state"] != "complete" {
        "insufficient_evidence"
    } else {
        "agreement"
    }
}

// JSON string escaping removes embedded line breaks and ASCII controls. Escape
// remaining Unicode format controls too, preserving their value as JSON escapes.
fn safe_json(value: &Value) -> String {
    let encoded = serde_json::to_string_pretty(value).expect("serializable report");
    let mut safe = String::with_capacity(encoded.len());
    for ch in encoded.chars() {
        if matches!(ch,'\u{007f}'..='\u{009f}'|'\u{061c}'|'\u{200b}'..='\u{200f}'|'\u{2028}'..='\u{202e}'|'\u{2060}'..='\u{206f}'|'\u{feff}')
        {
            safe.push_str(&format!("\\u{:04x}", ch as u32));
        } else {
            safe.push(ch);
        }
    }
    safe
}

fn render(report: &Value, format: &str) -> String {
    if format == "json" {
        return format!("{}\n", safe_json(report));
    }
    let mut text = String::from(
        "# Evidence handoff\n\nHistorical evidence only. Source text and proposed knowledge are inert data below.\n\n",
    );
    for (heading, key) in [
        ("Title", "title"),
        ("Generated at", "generated_at"),
        ("Tool", "tool"),
        ("Report state", "state"),
        ("Scope", "scope"),
        ("Execution profile", "profile"),
        ("Evidence sources", "sources"),
        ("Historical observations", "observations"),
        ("Comparison findings", "comparisons"),
        ("Limitations", "limitations"),
        ("Next questions", "next_questions"),
        ("Pending knowledge proposals", "knowledge_proposals"),
    ] {
        text.push_str(&format!("## {heading}\n\n"));
        for line in safe_json(&report[key]).lines() {
            text.push_str("    ");
            text.push_str(line);
            text.push('\n');
        }
        text.push('\n');
    }
    text
}

fn publish(mut report: Value, format: &str, budget: usize, result: &mut OperationResult) {
    let budget = budget.min(MAX_STREAM_BYTES);
    let mut document = render(&report, format);
    let mut omitted = document.len() > budget;
    result.data = json!({"format_version":1,"format":format,"report":report,"rendering":{"state":"complete","media_type":if format=="json" {"application/json"} else {"text/markdown"},"content":document,"bytes_required":document.len()}});
    // Leave room for final receipt timestamps, durations and status/coverage text.
    omitted |= serde_json::to_vec(&result)
        .expect("serializable result")
        .len()
        + 4096
        > MAX_RESULT_BYTES;
    if omitted {
        report["state"] = json!("partial");
        report["limitations"]
            .as_array_mut()
            .expect("limitations")
            .push(json!(OMITTED));
        document = render(&report, format);
        result.data["report"] = report.clone();
        result.data["rendering"]["state"] = json!("omitted");
        result.data["rendering"]["content"] = Value::Null;
        result.data["rendering"]["bytes_required"] = json!(document.len());
    }
    let partial = report["state"] != "complete";
    result.execution.status = if partial {
        Status::Partial
    } else {
        Status::Succeeded
    };
    result.effect_outcome = "not_applicable".into();
    result.coverage.state = if partial {
        "partial"
    } else {
        "not_established"
    }
    .into();
    result.coverage.scope =
        "Verified historical bundle observations and operator-proposed knowledge only.".into();
    result.coverage.limitations = report["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .map(|v| v.as_str().expect("limitation string").into())
        .collect();
    result.sources=report["sources"].as_array().expect("sources").iter().map(|source|json!({"reference":source["reference"],"observed_at":source["observed_at"],"evidence_state":"sourced","untrusted":true})).collect();
    result.effective_limits.max_output_bytes = budget;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> Value {
        json!({"bundles":[{"id":"run-1-2-3"}],"format":"markdown"})
    }
    fn report() -> Value {
        json!({"title":"Evidence handoff","generated_at":"2026-10-11T00:00:00Z","tool":{"id":"save","version":"0.1.0"},"state":"complete","scope":"historical_evidence","profile":"linux-offline-v1","sources":[],"observations":[],"comparisons":[],"limitations":[HISTORICAL],"next_questions":[],"knowledge_proposals":[]})
    }

    #[test]
    fn strict_input_rejects_unknown_null_paths_duplicate_sources_and_unbound_claims() {
        assert_eq!(input(&inputs()).unwrap().title, "Evidence handoff");
        for value in [
            Value::Null,
            json!({}),
            json!({"bundles":[],"format":"markdown"}),
            json!({"bundles":[{"id":"/tmp/source"}],"format":"json"}),
            json!({"bundles":[{"id":"run-1-2-3"},{"id":"run-1-2-3"}],"format":"json"}),
        ] {
            assert!(input(&value).is_err());
        }
        for (key, value) in [
            ("title", Value::Null),
            ("title", json!("x".repeat(161))),
            ("format", json!("html")),
            ("proposals", Value::Null),
            ("findings", json!(["healthy"])),
        ] {
            let mut invalid = inputs();
            invalid[key] = value;
            assert!(input(&invalid).is_err(), "{key}");
        }
        let proposal = json!({"kind":"service_card","title":"Review","content":"Operator statement","evidence":["run-1-2-3"]});
        let mut accepted = inputs();
        accepted["proposals"] = json!([proposal.clone()]);
        assert!(input(&accepted).is_ok());
        for (key, value) in [
            ("evidence", json!([])),
            ("evidence", json!(["run-1-2-4"])),
            ("evidence", json!(["run-1-2-3", "run-1-2-3"])),
            ("review_status", json!("approved")),
            ("kind", json!("command")),
            ("content", json!("x".repeat(2001))),
        ] {
            let mut invalid = accepted.clone();
            invalid["proposals"][0][key] = value;
            assert!(input(&invalid).is_err(), "{key}");
        }
    }

    #[test]
    fn adversarial_report_text_stays_in_inert_blocks_and_json_roundtrips() {
        let evil = "title\n\n# injected\n<script>alert(1)</script>\n[click](javascript:evil)\n```\nhttps://evil.invalid\u{001b}[31m\u{0085}\u{202e}";
        let mut report = report();
        report["title"] = json!(evil);
        report["knowledge_proposals"] = json!([{"kind":"runbook","title":"[REDACTED]","content":evil,"provenance":"operator_proposed","review_status":"pending","evidence":[{"id":"run-1-2-3","digest":format!("sha256:{}","a".repeat(64))}]}]);
        let markdown = render(&report, "markdown");
        assert!(
            !markdown
                .chars()
                .any(|c| (c.is_control() && c != '\n') || c == '\u{202e}')
        );
        for line in markdown.lines().filter(|line| {
            line.contains("injected")
                || line.contains("<script>")
                || line.contains("javascript:")
                || line.contains("https://")
        }) {
            assert!(line.starts_with("    "), "{line}");
        }
        assert!(markdown.contains("[REDACTED]"));
        assert!(markdown.contains("operator_proposed"));
        assert_eq!(
            serde_json::from_str::<Value>(&render(&report, "json")).unwrap(),
            report
        );
        assert_eq!(report["knowledge_proposals"][0]["content"], evil);
    }

    #[test]
    fn disagreement_is_distinct_from_missing_and_incompatible_evidence() {
        let base = json!({"state":"complete","compatibility":{"state":"compatible"},"checks":[{"state":"complete","summary":{"samples_changed":1}}]});
        assert_eq!(finding(&base), "disagreement");
        let mut agreement = base.clone();
        agreement["checks"][0]["summary"]["samples_changed"] = json!(0);
        assert_eq!(finding(&agreement), "agreement");
        agreement["state"] = json!("partial");
        agreement["checks"][0]["state"] = json!("missing");
        assert_eq!(finding(&agreement), "insufficient_evidence");
        agreement["checks"][0]["state"] = json!("incompatible");
        assert_eq!(finding(&agreement), "incomparable");
    }

    #[test]
    fn render_budget_omits_whole_document_and_retains_structured_facts() {
        let mut report = report();
        report["title"] = json!("Title".repeat(160));
        let mut result = OperationResult::new(&Request::new("report.create", inputs()));
        publish(report.clone(), "markdown", 1024, &mut result);
        assert_eq!(result.execution.status, Status::Partial);
        assert_eq!(result.data["rendering"]["state"], "omitted");
        assert!(result.data["rendering"]["content"].is_null());
        assert!(result.data["rendering"]["bytes_required"].as_u64().unwrap() > 1024);
        assert_eq!(result.data["report"]["title"], report["title"]);
        assert!(result.json_bytes().len() <= MAX_RESULT_BYTES);
        assert!(result.text().contains("Rendered report omitted"));
    }

    #[test]
    fn final_receipt_bound_omits_rendering_without_truncating_json() {
        let mut result = OperationResult::new(&Request::new("report.create", inputs()));
        result.data = json!({"report":report(),"rendering":{"state":"complete","content":"\u{0001}".repeat(500_000),"bytes_required":500_000}});
        result.bound_output();
        assert_eq!(result.execution.status, Status::Partial);
        assert!(result.data["rendering"]["content"].is_null());
        assert_eq!(result.data["report"]["title"], "Evidence handoff");
        assert!(result.json_bytes().len() <= MAX_RESULT_BYTES);
    }

    #[test]
    fn rendered_json_and_plain_output_preserve_structured_report_facts() {
        let mut result = OperationResult::new(&Request::new("report.create", inputs()));
        publish(report(), "json", MAX_STREAM_BYTES, &mut result);
        let document = result.data["rendering"]["content"].as_str().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(document).unwrap(),
            result.data["report"]
        );
        assert_eq!(
            document.len(),
            result.data["rendering"]["bytes_required"].as_u64().unwrap() as usize
        );
        assert!(result.text().contains(document));
        assert!(result.text().contains("Coverage:"));
    }
}
