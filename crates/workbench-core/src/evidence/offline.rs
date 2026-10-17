//! Read-only inspection and deterministic projection of historical evidence.
pub(super) mod profile;

use super::{
    compare, frames,
    store::{EvidenceStoreOptions, Store, StoreError, VerifiedBundle},
};
use crate::{
    OperatorContext, RunControl,
    request::{Problem, Request, parse_bounded_json},
    result::{MAX_RECEIPT_DEPTH, OperationResult, Status, timestamp},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

fn stopped(result: &mut OperationResult, code: &'static str, control: &RunControl) {
    result.execution.status = if code == "deadline_exceeded" {
        Status::TimedOut
    } else {
        Status::Cancelled
    };
    if result.data.get("state").is_some() {
        result.data["state"] = json!("partial");
    }
    if result.data.is_object() {
        result.data["cancellation_signal"] = json!(control.signal.load(Ordering::Relaxed));
    }
    result.coverage.state = "partial".into();
    result.error(
        code,
        "Offline analysis stopped before all historical observations were inspected.",
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

pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    if let Err(problem) = super::reference(&request.inputs) {
        result.reject(problem);
        return;
    }
    let Some(options) = &context.evidence_store else {
        result.reject(Problem::invalid(
            "evidence_store_required",
            "Attach an explicit private --state-dir before opening or replaying evidence.",
        ));
        return;
    };
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    if let Err(code) = compare::active(control, deadline) {
        stopped(result, code, control);
        return;
    }
    // The worker receives only the explicit store policy, request and control.
    // No Grafana/context configuration or credential provider crosses this seam.
    let worker = std::thread::scope(|scope| {
        std::thread::Builder::new().name("evidence-offline".into()).spawn_scoped(scope, || {
            if let Err(code) = profile::install() {
                result.reject(Problem::unsupported(code, "Offline analysis requires the Linux x86_64 seccomp execution profile; no evidence was opened."));
                return;
            }
            analyze(request, control, options, deadline, result);
        }).map(|worker| worker.join())
    });
    if !matches!(worker, Ok(Ok(()))) {
        result.reject(Problem::unsupported(
            "offline_worker_failed",
            "The bounded offline analysis worker could not complete.",
        ));
    }
}

fn analyze(
    request: &Request,
    control: &RunControl,
    options: &EvidenceStoreOptions,
    deadline: Instant,
    result: &mut OperationResult,
) {
    let check = || compare::active(control, deadline);
    if let Err(code) = check() {
        stopped(result, code, control);
        return;
    }
    let store = match Store::open_validated(
        options,
        false,
        deadline,
        super::validate_bundle,
        Some(Arc::clone(&control.signal)),
    ) {
        Ok(store) => store,
        Err(error) => {
            store_failure(result, error, control);
            return;
        }
    };
    let (id, pin) = super::reference(&request.inputs).expect("validated reference");
    let bundle = match store.load_verified(id) {
        Ok(bundle) => bundle,
        Err(error) => {
            store_failure(result, error, control);
            return;
        }
    };
    if pin.is_some_and(|pin| pin != bundle.reference.digest) {
        super::fail(result, StoreError::new("manifest_digest_mismatch", false));
        return;
    }
    analyze_bundle(request, control, deadline, result, &bundle);
}

pub(super) fn analyze_bundle(
    request: &Request,
    control: &RunControl,
    deadline: Instant,
    result: &mut OperationResult,
    bundle: &VerifiedBundle,
) {
    let mut check = || compare::active(control, deadline);
    result.effect_outcome = "not_applicable".into();
    let p = &bundle.manifest.payload;
    let mode = if request.operation == "evidence.open" {
        "open"
    } else {
        "replay"
    };
    let mut source_limitations = Vec::new();
    if bundle.manifest.state != "sealed" {
        source_limitations.push("interrupted_source_bundle");
    }
    if p["capture"]["state"] != "complete" {
        source_limitations.push("partial_source_capture");
    }
    let uninspected_artifacts = bundle
        .artifacts
        .keys()
        .filter(|id| !matches!(id.as_str(), "metrics" | "logs"))
        .count() as u64;
    if uninspected_artifacts != 0 {
        source_limitations.push("uninspected_recovered_artifacts");
    }
    let identity_reasons = compare::identities(p, p);
    let mut partial = !source_limitations.is_empty() || !identity_reasons.is_empty();
    result.data = json!({
        "format_version":1,"mode":mode,"algorithm":{"id":"retained-observations-v1","version":1},
        "analysis":{"at":timestamp(),"tool":"save","version":env!("CARGO_PKG_VERSION"),"profile":"linux-offline-v1"},
        "bundle":bundle.reference,
        "observation":{"started_at":p["started_at"],"finalized_at":p["finalized_at"],"window":p["window"],
            "selection":p["selection"],"manifest_state":bundle.manifest.state,"capture":p["capture"]},
        "state":"partial","source_limitations":source_limitations,"checks":[],"omitted_details":0
    });
    if mode == "open" {
        result.data["manifest"] = json!(bundle.manifest);
    }
    result.sources = vec![
        json!({"reference":format!("evidence-bundle:{}@{}", bundle.reference.id, bundle.reference.digest),
        "observed_at":p["started_at"],"evidence_state":"sourced","untrusted":true}),
    ];
    result.coverage.scope = "Historical observations in the verified local bundle only.".into();
    result.coverage.limitations.push("Integrity verification does not establish source truth or authorship. Historical observations are not current health; source coverage, absent telemetry and causation remain unestablished.".into());
    let mut count = request.limits.max_items.unwrap_or(100).min(1000) as usize;
    let mut bytes = request.limits.max_output_bytes.min(1024 * 1024);
    result.effective_limits.max_items = Some(count as u64);
    result.effective_limits.max_output_bytes = bytes;
    let mut omitted = if mode == "open" {
        uninspected_artifacts
    } else {
        0
    };
    result.data["omitted_details"] = json!(omitted);
    for (index, (id, kind, unit)) in [
        ("metrics", "prometheus", "dimensionless"),
        ("logs", "loki", "entries"),
    ]
    .into_iter()
    .enumerate()
    {
        if let Err(code) = check() {
            stopped(result, code, control);
            return;
        }
        let step = p["steps"]
            .as_array()
            .and_then(|steps| steps.iter().find(|step| step["id"] == id));
        let mut output = json!({"id":id,"kind":kind,"unit":unit,"definition":step.map(|s| &s["definition"]),
            "state":"complete","reasons":[],"summary":{},"details":[],"omitted_details":0});
        let analyzed = (|| {
            if let Some(reason) = identity_reasons.first() {
                return Err(*reason);
            }
            let observation = compare::observation(bundle, id, kind)?;
            if mode == "open" {
                let mut reasons = frames::inspect(
                    kind,
                    &observation.data["response"]["results"]["A"],
                    compare::window(p)?,
                    &mut check,
                )?;
                if observation.partial {
                    reasons.push("partial_source_receipt");
                }
                let partial = !reasons.is_empty();
                output["reasons"] = json!(reasons);
                return Ok(partial);
            }
            let projection = frames::replay(
                kind,
                &observation.data["response"]["results"]["A"],
                compare::window(p)?,
                count,
                bytes.saturating_sub((1 - index) * 2),
                &mut check,
            )?;
            output["summary"] = projection.summary;
            output["details"] = json!(projection.details);
            output["reasons"] = json!(projection.reasons);
            output["omitted_details"] = json!(projection.omitted_details);
            if observation.partial {
                output["reasons"]
                    .as_array_mut()
                    .expect("reasons")
                    .push(json!("partial_source_receipt"));
            }
            Ok(projection.partial || observation.partial)
        })();
        match analyzed {
            Ok(is_partial) => {
                if is_partial {
                    output["state"] = json!("partial");
                }
                partial |= is_partial;
            }
            Err(code) => {
                if matches!(
                    code,
                    "cancelled" | "deadline_exceeded" | "result_consumer_closed"
                ) {
                    stopped(result, code, control);
                    return;
                }
                output["state"] = json!(if matches!(
                    code,
                    "missing_check" | "unusable_receipt" | "missing_response" | "unusable_response"
                ) {
                    "missing"
                } else {
                    "incompatible"
                });
                output["reasons"] = json!([code]);
                partial = true;
            }
        }
        if mode == "open"
            && let Err(code) = open_receipt(
                bundle,
                id,
                count,
                bytes.saturating_sub((1 - index) * 2),
                &mut output,
            )
        {
            output["state"] = json!("incompatible");
            output["reasons"]
                .as_array_mut()
                .expect("reasons")
                .push(json!(code));
            partial = true;
        }
        let omitted_here = output["omitted_details"].as_u64().expect("omitted count");
        omitted += omitted_here;
        if omitted_here != 0 {
            count = 0;
            partial = true;
            if output["state"] == "complete" {
                output["state"] = json!("partial");
            }
            if mode == "open" {
                output["reasons"]
                    .as_array_mut()
                    .expect("reasons")
                    .push(json!("details_omitted"));
            }
        }
        count = count.saturating_sub(output["details"].as_array().expect("details").len());
        bytes = bytes.saturating_sub(
            serde_json::to_vec(&output["details"])
                .expect("serializable details")
                .len(),
        );
        result.data["checks"]
            .as_array_mut()
            .expect("checks")
            .push(output);
        result.data["omitted_details"] = json!(omitted);
    }
    if let Err(code) = check() {
        stopped(result, code, control);
        return;
    }
    result.data["state"] = json!(if partial { "partial" } else { "complete" });
    result.execution.status = if partial {
        Status::Partial
    } else {
        Status::Succeeded
    };
    result.coverage.state = if partial {
        "partial"
    } else {
        "not_established"
    }
    .into();
}

fn open_receipt(
    bundle: &VerifiedBundle,
    id: &str,
    count: usize,
    bytes: usize,
    output: &mut Value,
) -> Result<(), &'static str> {
    let Some(retained) = bundle.artifacts.get(id) else {
        return Ok(());
    };
    let receipt = parse_bounded_json(retained, MAX_RECEIPT_DEPTH).map_err(|_| "invalid_receipt")?;
    output["summary"] = json!({"retained_receipts":1});
    let detail = json!({"type":"receipt","receipt":receipt});
    let encoded = serde_json::to_vec(&detail)
        .map_err(|_| "invalid_receipt")?
        .len()
        + 2;
    if count != 0 && encoded <= bytes {
        output["details"] = json!([detail]);
    } else {
        output["omitted_details"] = json!(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::store::{BundleRef, Manifest};
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn cancellation_keeps_signal_and_partial_analysis() {
        let mut result =
            OperationResult::new(&Request::new("evidence.open", json!({"id":"run-1-2-3"})));
        result.data = json!({"state":"complete"});
        let control = RunControl::default();
        control.signal.store(2, Ordering::Relaxed);
        stopped(&mut result, "cancelled", &control);
        assert_eq!(result.execution.status, Status::Cancelled);
        assert_eq!(result.data["state"], "partial");
        assert_eq!(result.data["cancellation_signal"], 2);
    }

    #[test]
    fn open_preserves_original_receipt_and_counts_omitted_receipts() {
        let receipt = json!({"operation":"query.run","started_at":"2026-01-01T00:00:00.000000123Z",
            "data":{"line":"$(touch /tmp/do-not-execute)","redacted":"[REDACTED]"}});
        let bundle = VerifiedBundle {
            reference: BundleRef {
                id: "run-1-2-3".into(),
                digest: format!("sha256:{}", "a".repeat(64)),
            },
            manifest: Manifest {
                format_version: 1,
                id: "run-1-2-3".into(),
                state: "sealed".into(),
                created_at_unix_ms: 0,
                finalized_at_unix_ms: 1,
                artifacts: BTreeMap::new(),
                payload: json!({}),
            },
            artifacts: BTreeMap::from([("metrics".into(), serde_json::to_vec(&receipt).unwrap())]),
            protected: false,
        };
        let mut output = json!({"details":[],"omitted_details":0});
        open_receipt(&bundle, "metrics", 1, 10000, &mut output).unwrap();
        assert_eq!(output["details"][0]["receipt"], receipt);
        assert_eq!(output["omitted_details"], 0);
        for (count, bytes) in [(0, 10000), (1, 2)] {
            let mut output = json!({"details":[],"omitted_details":0});
            open_receipt(&bundle, "metrics", count, bytes, &mut output).unwrap();
            assert_eq!(output["summary"]["retained_receipts"], 1);
            assert_eq!(output["omitted_details"], 1);
            assert_eq!(output["details"], json!([]));
        }
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn profile_store_read_does_not_create_or_mutate_store_entries() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(crate::result::new_id("offline-store"));
        std::fs::create_dir(&base).unwrap();
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
        let options = EvidenceStoreOptions::new(base.join("store"));
        drop(Store::open(&options, true, Instant::now() + Duration::from_secs(30)).unwrap());
        let snapshot = || {
            let mut files = BTreeMap::new();
            for entry in std::fs::read_dir(&options.directory).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_file() {
                    files.insert(entry.file_name(), std::fs::read(entry.path()).unwrap());
                }
            }
            files
        };
        let before = snapshot();
        let context = OperatorContext {
            evidence_store: Some(options.clone()),
            ..Default::default()
        };
        for operation in ["evidence.open", "evidence.replay"] {
            let result = crate::execute_with_context(
                Request::new(operation, json!({"id":"run-1-2-3"})),
                &RunControl::default(),
                &context,
            );
            assert_eq!(result.errors[0].code, "store_not_found");
            assert_eq!(snapshot(), before);
            assert_eq!(
                std::fs::read_dir(options.directory.join("runs"))
                    .unwrap()
                    .count(),
                0
            );
        }
        std::fs::remove_dir_all(base).unwrap();
    }
}
