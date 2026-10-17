use super::*;
use package::Package;
use policy::Policy;
use std::collections::BTreeMap;
fn entry(
    package: &Package,
    capability: &Value,
    policy: Option<&Policy>,
    counts: &BTreeMap<String, usize>,
) -> Value {
    let operation = capability["operation"].as_str().unwrap();
    let authorized = policy.is_some_and(|p| p.grants(package, operation));
    let ambiguous = authorized && counts.get(operation).is_some_and(|n| *n > 1);
    let available =
        cfg!(all(target_arch = "x86_64", target_pointer_width = "64")) && authorized && !ambiguous;
    json!({"capability":{"spec_version":"0.1","id":operation,"operation_version":1,"title":format!("Installed extension {operation}"),
        "description":"External package capability under separately pinned operator-reviewed authority. Package declarations, provenance and outputs are untrusted; declared observe does not establish absence of effects.",
        "effects":"unclassified","input_schema":capability["input_schema"],"output_schema":{"$ref":"urn:sre-workbench:spec:result:0.1"},
        "execution_modes":["foreground"],"platforms":["linux"],"dependencies":[],"stability":"experimental","default_timeout_ms":30000,"max_timeout_ms":300000,
        "required_permissions":["extension.execute"],"implementation":{"kind":"executable","reference":format!("extension-package:{}#{}",package.digest,operation),"digest":package.digest}},
        "availability":{"status":if !cfg!(all(target_arch="x86_64",target_pointer_width="64")){"unsupported_platform"}else if available{"available"}else{"requires_configuration"},
        "reason":if ambiguous{"Multiple policy-pinned packages authorize this operation; execution is refused as ambiguous."}else if available{"Exact installed package and capability are authorized by the startup policy under ordinary operator authority."}else{"Requires a valid startup execution policy pinning this exact package and capability."}}})
}
pub(super) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    if let Err(fault) = perform(request, control, context, deadline, result) {
        failure(fault, control, result);
    }
}
fn perform(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    deadline: Instant,
    result: &mut OperationResult,
) -> Result<(), Fault> {
    active(control, deadline)?;
    let policy = Policy::load(context, control, deadline)?;
    let path = context
        .extension_state_dir
        .as_deref()
        .ok_or_else(|| Fault::new("extension_store_required"))?;
    let store = storage::Store::open(path, false, control, deadline)?;
    let counts = policy::counts(&store.packages, policy.as_ref());
    let mut entries = Vec::new();
    for package in &store.packages {
        for capability in package.manifest["capabilities"].as_array().unwrap() {
            if request.operation == "capability.list"
                || capability["operation"] == request.inputs["operation"]
            {
                entries.push((
                    policy.as_ref().is_some_and(|p| {
                        p.grants(package, capability["operation"].as_str().unwrap())
                    }),
                    entry(package, capability, policy.as_ref(), &counts),
                ));
            }
        }
    }
    entries.sort_by(|a, b| {
        a.1["capability"]["id"]
            .as_str()
            .cmp(&b.1["capability"]["id"].as_str())
            .then_with(|| {
                a.1["capability"]["implementation"]["digest"]
                    .as_str()
                    .cmp(&b.1["capability"]["implementation"]["digest"].as_str())
            })
    });
    if request.operation == "capability.describe" {
        if entries.is_empty() {
            return Err(Fault::new("extension_operation_not_found"));
        }
        let allowed = entries.iter().filter(|(grant, _)| *grant).count();
        if allowed > 1 || (allowed == 0 && entries.len() > 1) {
            return Err(Fault::new("extension_ambiguous_operation"));
        }
        if allowed == 1 {
            entries.retain(|(grant, _)| *grant);
        }
    }
    let total = entries.len();
    let mut rows: Vec<_> = entries.into_iter().map(|(_, row)| row).collect();
    while serde_json::to_vec(&rows).expect("metadata").len()
        > result.effective_limits.max_output_bytes
    {
        if rows.pop().is_none() {
            break;
        }
    }
    let omitted = total - rows.len();
    if request.operation == "capability.list" {
        result.data["capabilities"]
            .as_array_mut()
            .unwrap()
            .extend(rows);
    } else {
        result.data=rows.pop().unwrap_or_else(||json!({"capability":null,"availability":{"status":"requires_configuration","reason":"Capability description omitted by the metadata budget."}}));
    }
    result.data["extension_discovery"] = json!({"policy_sha256":policy.as_ref().map(|p|&p.digest),"verified_packages":store.packages.len(),"total_capabilities":total,"omitted_capabilities":omitted});
    result.effect_outcome = "not_applicable".into();
    result.coverage.state = if omitted > 0 { "partial" } else { "complete" }.into();
    if omitted > 0 {
        result.execution.status = Status::Partial;
        result.coverage.limitations.push("External capability descriptions omitted by the metadata budget; builtin descriptors are retained.".into());
    }
    active(control, deadline)
}
