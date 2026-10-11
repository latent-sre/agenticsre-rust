//! Operator-local collection and durable evidence. Hosted grants remain separate.
pub(crate) mod collect;
pub(crate) mod pack;
pub(crate) mod store;

pub use store::EvidenceStoreOptions;

use crate::{
    OperatorContext, RunControl,
    request::{Problem, Request},
    result::{OperationResult, Status},
};
use serde_json::{Value, json};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use store::{Store, StoreError, VerifiedBundle};

fn reference(inputs: &Value) -> Result<(&str, Option<&str>), Problem> {
    let invalid = || {
        Problem::invalid(
            "invalid_evidence_reference",
            "Supply a generated run id and, optionally, its sha256: manifest digest; paths are not accepted.",
        )
    };
    let object = inputs.as_object().ok_or_else(invalid)?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "id" | "expected_manifest_sha256"))
    {
        return Err(invalid());
    }
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| store::valid_run_id(id))
        .ok_or_else(invalid)?;
    let expected = match object.get("expected_manifest_sha256") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .filter(|value| value.strip_prefix("sha256:").is_some_and(pack::digest))
                .ok_or_else(invalid)?,
        ),
    };
    Ok((id, expected))
}

/// Envelope, artifact and domain identity agree before a bundle is displayed or used.
pub(crate) fn validate_bundle(bundle: &VerifiedBundle) -> Result<(), StoreError> {
    let corrupt = || StoreError::new("store_corrupt", false);
    let manifest = &bundle.manifest;
    if manifest.payload["format_version"] != 1
        || manifest.payload["producer"]["spec_version"] != "0.1"
    {
        return Err(StoreError::new("store_unsupported_payload", false));
    }
    // Only the store can construct this in-memory deletion preview, after checking the
    // original on-disk envelope. Its journal keeps the original manifest bytes/digest.
    let deleting = manifest.state == "deleting";
    let interrupted = manifest.state == "interrupted" || deleting;
    collect::validate_payload(&manifest.payload).map_err(|_| corrupt())?;
    if manifest.payload["run_id"] != manifest.id {
        return Err(corrupt());
    }
    if manifest.state == "sealed" && manifest.payload["finalized_at"].is_null() {
        return Err(corrupt());
    }
    let steps = manifest.payload["steps"].as_array().ok_or_else(corrupt)?;
    let mut references = std::collections::BTreeSet::new();
    for step in steps {
        let id = step["id"].as_str().ok_or_else(corrupt)?;
        if !step["artifact"].is_null() {
            let artifact: store::ArtifactRef =
                serde_json::from_value(step["artifact"].clone()).map_err(|_| corrupt())?;
            if manifest.artifacts.get(id) != Some(&artifact)
                || (!deleting && !bundle.artifacts.contains_key(id))
            {
                return Err(corrupt());
            }
            references.insert(id);
        } else if manifest.artifacts.contains_key(id) {
            // A crash can leave a newer verified reference than its payload checkpoint.
            if !interrupted {
                return Err(corrupt());
            }
            references.insert(id);
        }
        if let Some(bytes) = bundle.artifacts.get(id) {
            collect::validate_receipt(&manifest.payload, id, bytes, interrupted)
                .map_err(|_| corrupt())?;
        }
    }
    if manifest.artifacts.keys().any(|key| {
        !references.contains(key.as_str())
            && !(interrupted && key.strip_prefix("recovered-").is_some_and(pack::digest))
    }) {
        return Err(corrupt());
    }
    Ok(())
}

fn fail(result: &mut OperationResult, error: StoreError) {
    result.execution.status = match error.code {
        "store_deadline_exceeded" => Status::TimedOut,
        "store_cancelled" => Status::Cancelled,
        _ => Status::Failed,
    };
    if error.effects {
        result.effect_outcome = "unknown".into();
    }
    result.error(
        error.code,
        "The evidence store operation could not be completed; no successful write is implied.",
    );
}

pub(crate) fn manage(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    let needs_reference = matches!(
        request.operation.as_str(),
        "evidence.show" | "evidence.protect" | "evidence.unprotect" | "evidence.delete"
    );
    let selected = if needs_reference {
        match reference(&request.inputs) {
            Ok(reference) => Some(reference),
            Err(problem) => {
                result.reject(problem);
                return;
            }
        }
    } else if request.inputs != json!({}) {
        result.reject(Problem::invalid(
            "invalid_evidence_input",
            "This evidence operation accepts an empty input object only.",
        ));
        return;
    } else {
        None
    };
    let Some(options) = &context.evidence_store else {
        result.reject(Problem::invalid(
            "evidence_store_required",
            "Attach an explicit private --state-dir before accessing evidence.",
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
    let signal = control.signal.load(Ordering::Relaxed);
    if signal != 0 {
        result.execution.status = Status::Cancelled;
        result.data = json!({"cancellation_signal":signal});
        result.error(
            "cancelled",
            "The store operation was cancelled before access.",
        );
        return;
    }
    let mut store = match Store::open_validated(
        options,
        false,
        deadline,
        validate_bundle,
        Some(std::sync::Arc::clone(&control.signal)),
    ) {
        Ok(store) => store,
        Err(error) => {
            fail(result, error);
            let signal = control.signal.load(Ordering::Relaxed);
            if signal != 0 {
                result.data["cancellation_signal"] = json!(signal);
            }
            return;
        }
    };
    let outcome = (|| -> Result<Value, StoreError> {
        if let Some((id, expected)) = selected {
            let bundle = match store.load_verified(id) {
                Ok(bundle) => bundle,
                Err(error)
                    if error.code == "store_deleting" && request.operation == "evidence.delete" =>
                {
                    let reference = store.deletion_reference(id)?;
                    if expected.is_some_and(|expected| expected != reference.digest) {
                        return Err(StoreError::new("manifest_digest_mismatch", false));
                    }
                    store.delete(id)?;
                    return Ok(json!({"bundle":reference,"deleted":true}));
                }
                Err(error) => return Err(error),
            };
            if expected.is_some_and(|expected| expected != bundle.reference.digest) {
                return Err(StoreError::new("manifest_digest_mismatch", false));
            }
            match request.operation.as_str() {
                "evidence.show" => {
                    let capture = if bundle.manifest.state == "interrupted" {
                        json!({"state":"partial","reason":"interrupted"})
                    } else {
                        json!({"state":bundle.manifest.payload["capture"]["state"],"reason":null})
                    };
                    Ok(
                        json!({"bundle":bundle.reference,"manifest":bundle.manifest,"protected":bundle.protected,"effective_capture":capture}),
                    )
                }
                "evidence.protect" | "evidence.unprotect" => {
                    let protected = request.operation == "evidence.protect";
                    store.protect(id, protected)?;
                    Ok(json!({"bundle":bundle.reference,"protected":protected}))
                }
                "evidence.delete" => {
                    store.delete(id)?;
                    Ok(json!({"bundle":bundle.reference,"deleted":true}))
                }
                _ => unreachable!("fixed reference operations"),
            }
        } else {
            match request.operation.as_str() {
                "evidence.list" => Ok(json!({"runs":store.list()?})),
                "evidence.prune" => {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| StoreError::new("invalid_clock", false))?
                        .as_millis();
                    let expired = store.prune(
                        u64::try_from(now).map_err(|_| StoreError::new("invalid_clock", false))?,
                    )?;
                    Ok(json!({"expired":expired}))
                }
                "evidence.recover" => Ok(json!({"recovered":store.recover()?})),
                _ => unreachable!("fixed store management operations"),
            }
        }
    })();
    match outcome {
        Ok(data) => {
            result.data = data;
            result.effect_outcome = if matches!(
                request.operation.as_str(),
                "evidence.list" | "evidence.show"
            ) {
                "not_applicable"
            } else {
                "succeeded"
            }
            .into();
            result.coverage.scope = "Owned local evidence bytes only; integrity is not source authenticity or service health.".into();
        }
        Err(error) => {
            result.data = json!({"completed":store.progress()});
            fail(result, error);
            let signal = control.signal.load(Ordering::Relaxed);
            if signal != 0 {
                result.data["cancellation_signal"] = json!(signal);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_refuse_paths_unknown_fields_null_and_unbounded_digests() {
        let valid = json!({"id":"run-1-ab-c","expected_manifest_sha256":format!("sha256:{}", "a".repeat(64))});
        assert!(reference(&valid).is_ok());
        for value in [
            json!({"id":"../run-1-ab-c"}),
            json!({"id":"run-1-ab-c","expected_manifest_sha256":null}),
            json!({"id":"run-1-ab-c","path":"elsewhere"}),
            json!({"id":"run-1-ab-c","expected_manifest_sha256":"a".repeat(64)}),
            json!(["run-1-ab-c"]),
        ] {
            assert!(reference(&value).is_err(), "{value}");
        }
    }
}
