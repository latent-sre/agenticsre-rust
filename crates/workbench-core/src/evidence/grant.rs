//! Explicit collection authority, distinct from offline investigation and generic hosted execution.
use crate::{
    GrafanaConfigIdentity, OperatorContext, RunControl, context,
    request::{Problem, Request},
    result::{OperationResult, Status},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::atomic::Ordering, time::Instant};

pub struct EvidenceCollectionGrant {
    targets: BTreeSet<String>,
    grafana: GrafanaConfigIdentity,
    source: context::ContextSourceIdentity,
    context: OperatorContext,
}

/// Admit only existing resources; this neither loads credentials nor initializes a store.
pub fn evidence_collection_grant(
    context: &OperatorContext,
    targets: &[String],
) -> Result<EvidenceCollectionGrant, Problem> {
    let invalid = || {
        Problem::invalid(
            "invalid_collection_grant",
            "Collection requires distinct configured targets, an explicit context source and age policy, and an existing private store.",
        )
    };
    let selected: BTreeSet<_> = targets.iter().cloned().collect();
    if selected.is_empty() || selected.len() != targets.len() {
        return Err(invalid());
    }
    let path = context.config_path.as_ref().ok_or_else(invalid)?;
    let (description, grafana) = crate::grafana_config_description(path)?;
    if selected.iter().any(|id| {
        !description
            .targets
            .iter()
            .any(|target| &target.id == id && target.api_profile.is_some())
    }) {
        return Err(invalid());
    }
    let source_path = context.context_source_path.as_ref().ok_or_else(invalid)?;
    let days = context
        .context_max_age_days
        .filter(|days| (1..=365).contains(days))
        .ok_or_else(invalid)?;
    let source = context::source_identity(source_path)?;
    let store = context
        .evidence_store
        .as_ref()
        .ok_or_else(invalid)?
        .pinned()
        .map_err(|error| {
            Problem::invalid(
                error.code,
                "The existing private evidence store could not be admitted.",
            )
        })?;
    Ok(EvidenceCollectionGrant {
        targets: selected,
        grafana,
        source,
        context: OperatorContext {
            config_path: Some(path.clone()),
            context_source_path: Some(source_path.clone()),
            context_max_age_days: Some(days),
            evidence_store: Some(store),
            ..OperatorContext::default()
        },
    })
}

pub fn normalize_evidence_collection_input(inputs: &Value) -> Result<Value, Problem> {
    super::collect::normalize_input(inputs)
}

/// The opaque grant can collect only its fixed pack; it conveys no management or offline grant.
pub fn execute_with_evidence_collection_grant(
    request: Request,
    control: &RunControl,
    grant: &EvidenceCollectionGrant,
) -> OperationResult {
    let start = Instant::now();
    let mut result = OperationResult::new(&request);
    if request.operation != "evidence.collect" || !grant.targets.contains(&request.target.id) {
        result.reject(Problem::unsupported(
            "collection_operation_not_granted",
            "Only fixed collection against the explicitly selected targets is granted.",
        ));
    } else if let Err(problem) = request.validate() {
        result.reject(problem);
    } else if control.signal.load(Ordering::Relaxed) != 0 {
        result.execution.status = Status::Cancelled;
        result.data = json!({"cancellation_signal":control.signal.load(Ordering::Relaxed)});
        result.error("cancelled", "The invocation was cancelled before dispatch.");
    } else {
        super::collect::run_pinned(
            &request,
            control,
            &grant.context,
            start,
            &mut result,
            Some(super::collect::CollectionIdentities {
                grafana: &grant.grafana,
                source: &grant.source,
            }),
        );
    }
    result.finish(start);
    result
}

/// Embedded metadata only. Generic hosted execution continues to refuse all evidence operations.
pub fn execute_evidence_catalog(request: Request, control: &RunControl) -> OperationResult {
    if matches!(
        request.operation.as_str(),
        "evidence.packs.list" | "evidence.packs.describe"
    ) {
        crate::execute(request, control)
    } else {
        let start = Instant::now();
        let mut result = OperationResult::new(&request);
        result.reject(Problem::unsupported(
            "evidence_catalog_scope",
            "Only embedded evidence pack metadata is available.",
        ));
        result.finish(start);
        result
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::EvidenceStoreOptions;
    use std::{fs, time::Duration};

    #[test]
    fn collection_grant_never_grants_management_or_generic_hosted_execution() {
        let directory = std::env::temp_dir().join(crate::result::new_id("collection-grant"));
        fs::create_dir(&directory).unwrap();
        let config = directory.join("config.json");
        fs::write(&config, json!({"spec_version":"0.1","record":"never","connections":{"fixture":{
            "origin":"https://example.invalid","expected_org_id":7,"api_profile":"grafana-legacy-v1",
            "credential_ref":"env:SAVE_GRAFANA_UNUSED_COLLECTION_FIXTURE"}}}).to_string()).unwrap();
        let source = directory.join("context.json");
        fs::write(
            &source,
            include_bytes!("../../../../tests/fixtures/context/unique.json"),
        )
        .unwrap();
        let options = EvidenceStoreOptions::new(directory.join("store"));
        let context = OperatorContext {
            config_path: Some(config),
            context_source_path: Some(source),
            context_max_age_days: Some(30),
            evidence_store: Some(options.clone()),
            ..OperatorContext::default()
        };
        assert!(evidence_collection_grant(&context, &["fixture".into()]).is_err());
        assert!(
            !options.directory.exists(),
            "startup cannot initialize a missing store"
        );
        drop(
            super::super::store::Store::open(
                &options,
                true,
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap(),
        );
        let grant = evidence_collection_grant(&context, &["fixture".into()]).unwrap();
        let control = RunControl::default();
        for operation in [
            "evidence.delete",
            "evidence.protect",
            "evidence.list",
            "query.run",
            "process.exec",
        ] {
            let mut request = Request::new(operation, json!({}));
            request.target.kind = "connection".into();
            request.target.id = "fixture".into();
            let result = execute_with_evidence_collection_grant(request, &control, &grant);
            assert_eq!(result.errors[0].code, "collection_operation_not_granted");
        }
        let mut request = Request::new("evidence.collect", json!({}));
        request.target.kind = "connection".into();
        request.target.id = "fixture".into();
        request.record = "always".into();
        let generic = crate::execute_with_authority_identities(
            request.clone(),
            &control,
            &context,
            None,
            None,
        );
        assert_eq!(generic.errors[0].code, "evidence_grants_unavailable");
        let offline = crate::investigation_grant(&context, &["evidence.open".into()]).unwrap();
        assert!(!offline.allows("evidence.collect"));
        assert_eq!(
            crate::execute_with_investigation_grant(request.clone(), &control, &offline).errors[0]
                .code,
            "investigation_operation_not_granted"
        );
        assert_eq!(
            execute_evidence_catalog(request, &control).errors[0].code,
            "evidence_catalog_scope"
        );
        assert_eq!(
            fs::read_dir(options.directory.join("runs"))
                .unwrap()
                .count(),
            0
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
