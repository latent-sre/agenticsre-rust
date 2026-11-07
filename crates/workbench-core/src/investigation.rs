//! Explicit offline hosted authority. A grant retains no live target or credential configuration.
use crate::{
    EvidenceStoreOptions, OperatorContext, RunControl, context, evidence,
    request::{Problem, Request},
    result::{OperationResult, Status},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::atomic::Ordering, time::Instant};

const OPERATIONS: [&str; 6] = [
    "context.resolve",
    "evidence.list",
    "evidence.open",
    "evidence.replay",
    "evidence.compare",
    "report.create",
];

/// Admitted startup bindings and exact operation authority. Wire input cannot manufacture a grant.
pub struct InvestigationGrant {
    operations: BTreeSet<String>,
    source_identity: Option<context::ContextSourceIdentity>,
    context: OperatorContext,
}

impl InvestigationGrant {
    pub fn allows(&self, operation: &str) -> bool {
        self.operations.contains(operation)
    }
}

/// Admit only the fixed offline observations, validating existing startup resources without
/// creating store entries. Every invocation checks identities on the actual descriptors/bytes used.
pub fn investigation_grant(
    context: &OperatorContext,
    operations: &[String],
) -> Result<InvestigationGrant, Problem> {
    let selected: BTreeSet<_> = operations.iter().cloned().collect();
    if selected.is_empty()
        || selected.len() != operations.len()
        || selected
            .iter()
            .any(|operation| !OPERATIONS.contains(&operation.as_str()))
    {
        return Err(Problem::invalid(
            "invalid_investigation_grant",
            "Select distinct installed offline investigation operations only.",
        ));
    }
    let mut sanitized = OperatorContext::default();
    let source_identity = if selected.contains("context.resolve") {
        let path = context.context_source_path.as_ref().ok_or_else(|| {
            Problem::invalid(
                "context_source_required",
                "An offline context grant requires its explicit startup source.",
            )
        })?;
        let days = context
            .context_max_age_days
            .filter(|days| (1..=365).contains(days))
            .ok_or_else(|| {
                Problem::invalid(
                    "context_age_policy_required",
                    "The source grant requires a paired max-age policy from 1 through 365 days.",
                )
            })?;
        let identity = context::source_identity(path)?;
        sanitized.context_source_path = Some(path.clone());
        sanitized.context_max_age_days = Some(days);
        Some(identity)
    } else {
        None
    };
    if selected
        .iter()
        .any(|operation| operation != "context.resolve")
    {
        let options: &EvidenceStoreOptions = context.evidence_store.as_ref().ok_or_else(|| {
            Problem::invalid(
                "evidence_store_required",
                "Offline evidence grants require an existing private startup store.",
            )
        })?;
        sanitized.evidence_store = Some(options.pinned().map_err(|error| {
            Problem::invalid(
                error.code,
                "The existing private evidence store could not be admitted.",
            )
        })?);
    }
    Ok(InvestigationGrant {
        operations: selected,
        source_identity,
        context: sanitized,
    })
}

/// Pure validation and normalization shared by hosted admission and the dedicated executor.
pub fn normalize_investigation_input(operation: &str, inputs: &Value) -> Result<Value, Problem> {
    match operation {
        "context.resolve" => context::normalize_input(inputs),
        "evidence.list" if inputs == &json!({}) => Ok(json!({})),
        "evidence.list" => Err(Problem::invalid(
            "invalid_evidence_input",
            "Evidence listing accepts an empty input object only.",
        )),
        "evidence.open" | "evidence.replay" => {
            evidence::reference(inputs)?;
            Ok(inputs.clone())
        }
        "evidence.compare" => {
            evidence::compare::validate_input(inputs)?;
            Ok(inputs.clone())
        }
        "report.create" => evidence::report::normalize_input(inputs),
        _ => Err(Problem::unsupported(
            "investigation_operation_not_granted",
            "Only installed offline investigation operations are available through this grant.",
        )),
    }
}

/// Execute only fixed offline observations under sanitized, identity-pinned startup authority.
pub fn execute_with_investigation_grant(
    mut request: Request,
    control: &RunControl,
    grant: &InvestigationGrant,
) -> OperationResult {
    let start = Instant::now();
    let mut result = OperationResult::new(&request);
    if !grant.allows(&request.operation) {
        result.reject(Problem::unsupported(
            "investigation_operation_not_granted",
            "This operation was not explicitly admitted by the offline investigation grant.",
        ));
    } else if let Err(problem) = request.validate() {
        result.reject(problem);
    } else if let Err(problem) = normalize_investigation_input(&request.operation, &request.inputs)
        .map(|normalized| request.inputs = normalized)
    {
        result.reject(problem);
    } else if control.signal.load(Ordering::Relaxed) != 0 {
        result.execution.status = Status::Cancelled;
        result.data = json!({"cancellation_signal": control.signal.load(Ordering::Relaxed)});
        result.error("cancelled", "The invocation was cancelled before dispatch.");
    } else {
        match request.operation.as_str() {
            "context.resolve" => context::run_pinned(
                &request,
                control,
                &grant.context,
                grant.source_identity.as_ref(),
                start,
                &mut result,
            ),
            "evidence.list" => {
                evidence::manage(&request, control, &grant.context, start, &mut result)
            }
            "evidence.open" | "evidence.replay" => {
                evidence::offline::run(&request, control, &grant.context, start, &mut result)
            }
            "evidence.compare" => {
                evidence::compare::run(&request, control, &grant.context, start, &mut result)
            }
            "report.create" => {
                evidence::report::run(&request, control, &grant.context, start, &mut result)
            }
            _ => unreachable!("grant admits only fixed offline observations"),
        }
    }
    result.finish(start);
    result
}
