use super::*;
use model::{Admission, Observation, Target};
use std::{collections::BTreeSet, path::Path, sync::Arc};
use store::{Run, Store};
const CLEANUP: Duration = Duration::from_millis(2500);
const RPC: Duration = Duration::from_millis(1000);
trait Peer: Send + Sync {
    fn call(
        &self,
        method: &str,
        input: Value,
        expected: &Value,
        c: &RunControl,
        d: Instant,
    ) -> OperationResult;
}
impl Peer for crate::runner::PreparedClient {
    fn call(
        &self,
        method: &str,
        input: Value,
        expected: &Value,
        c: &RunControl,
        d: Instant,
    ) -> OperationResult {
        crate::runner::prepared_call(self, method, input, expected, c, d)
    }
}
type Client = Option<Arc<dyn Peer>>;
fn path(context: &OperatorContext) -> Result<&Path, Fault> {
    context
        .fleet_state_dir
        .as_deref()
        .ok_or(Fault::new("fleet_store_required"))
}
pub(super) fn run(
    request: &Request,
    c: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
    expected: Option<&Value>,
) -> Result<(), Fault> {
    let normalized = config::input(request)?;
    let deadline = start + Duration::from_millis(request.limits.timeout_ms);
    active(c, deadline)?;
    let path = path(context)?;
    if request.operation == "fleet.collect" {
        return collect(
            request, normalized, path, c, context, deadline, result, expected,
        );
    }
    let store = Store::open(path, false, c, deadline)?;
    if expected.is_some_and(|e| store.identity().ok().as_ref() != Some(&e["store"])) {
        return Err(Fault::new("fleet_namespace_changed"));
    }
    let mut run = store
        .find(normalized["request_id"].as_str().unwrap())?
        .ok_or(Fault::new("fleet_not_found"))?;
    if expected.is_some_and(|e| {
        run.admission.scheduler_binding_sha256.as_deref() != Some(&model::digest(e))
    }) {
        return Err(Fault::new("fleet_binding_changed"));
    }
    if request.operation == "fleet.status" {
        return respond(request, &run, "status", result);
    }
    if request.operation == "fleet.cancel"
        && !run.state.cancel_requested
        && run.state.phase != "terminal"
    {
        run.state.cancel_requested = true;
        run.state.updated_at = crate::result::timestamp();
        store.commit(&mut run)?;
        result.effect_outcome = "succeeded".into();
    }
    if run.state.phase == "terminal" {
        return respond(
            request,
            &run,
            if request.operation == "fleet.cancel" {
                "cancelled"
            } else {
                "resume"
            },
            result,
        );
    }
    let Some(lease) = store::claim(&run)? else {
        if request.operation == "fleet.cancel" {
            return respond(request, &run, "cancel_requested", result);
        }
        return Err(Fault::new("fleet_worker_busy"));
    };
    let clients = reload(&run.admission, c, deadline);
    drop(store);
    let action = if request.operation == "fleet.cancel" {
        "cancelled"
    } else {
        "resume"
    };
    if let Err(error) = drive(path, &mut run, clients, c, deadline, false, result) {
        run.state = run.durable_state.clone();
        if run.state.phase != "terminal" {
            run.state.phase = "recovery_required".into();
        }
        respond(request, &run, action, result)?;
        return Err(error.after_effects());
    }
    drop(lease);
    respond(request, &run, action, result)
}
// Keep expected authority explicit beside the existing controller boundary.
#[allow(clippy::too_many_arguments)]
fn collect(
    request: &Request,
    normalized: Value,
    path: &Path,
    c: &RunControl,
    context: &OperatorContext,
    deadline: Instant,
    result: &mut OperationResult,
    expected: Option<&Value>,
) -> Result<(), Fault> {
    let intent =
        model::digest(&json!({"inputs":normalized,"timeout_ms":request.limits.timeout_ms}));
    match Store::open(path, false, c, deadline) {
        Ok(store) => {
            if expected.is_some_and(|e| store.identity().ok().as_ref() != Some(&e["store"])) {
                return Err(Fault::new("fleet_namespace_changed"));
            }
            if let Some(run) = store.find(normalized["request_id"].as_str().unwrap())? {
                if run.admission.intent_sha256 != intent
                    || run.admission.scheduler_binding_sha256 != expected.map(model::digest)
                {
                    return Err(Fault::new("fleet_request_conflict"));
                }
                return respond(request, &run, "existing", result);
            }
        }
        Err(e) if e.code == "fleet_store_not_found" && expected.is_none() => {}
        Err(e) => return Err(e),
    }
    let id = crate::result::new_id("fleet");
    let (inventory_sha256, inventory_revision, selection_sha256, binding, targets, clients) =
        prepare(&normalized, context, &id, c, deadline)?;
    if let Some(expected) = expected
        && prepared_binding(&inventory_sha256, &selection_sha256, &binding, &targets)
            != expected["selection"]
    {
        return Err(Fault::new("fleet_binding_changed"));
    }
    let durable = RunControl::default();
    let store = Store::open(path, expected.is_none(), &durable, deadline + CLEANUP)?;
    if expected.is_some_and(|e| store.identity().ok().as_ref() != Some(&e["store"])) {
        return Err(Fault::new("fleet_namespace_changed"));
    }
    if store.created {
        result.effect_outcome = "succeeded".into();
    }
    if let Some(run) = store.find(normalized["request_id"].as_str().unwrap())? {
        if run.admission.intent_sha256 != intent
            || run.admission.scheduler_binding_sha256 != expected.map(model::digest)
        {
            return Err(Fault::new("fleet_request_conflict"));
        }
        return respond(request, &run, "existing", result);
    }
    let ordinal = store.ordinal(&selection_sha256)?;
    let now = time::OffsetDateTime::now_utc();
    let expires = now
        + time::Duration::milliseconds(
            deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as i64,
        );
    let mut admission = Admission {
        format_version: 1,
        scheduler_binding_sha256: expected.map(model::digest),
        id,
        request_id: normalized["request_id"].as_str().unwrap().into(),
        owner_uid: model::uid(),
        request_sha256: String::new(),
        intent_sha256: intent,
        inventory_sha256,
        inventory_revision,
        selection_sha256,
        task: normalized["task"].clone(),
        task_binding_sha256: binding,
        created_at: now
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
        expires_at: expires
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
        timeout_ms: request.limits.timeout_ms,
        concurrency: normalized["concurrency"].as_u64().unwrap() as usize,
        failure_threshold: normalized["failure_threshold"].as_u64().unwrap() as usize,
        ordinal,
        targets,
    };
    admission.request_sha256 = admission.fingerprint();
    let mut run = store.create(admission)?;
    result.effect_outcome = "succeeded".into();
    let lease = store::claim(&run)?.ok_or(Fault::new("fleet_worker_busy"))?;
    drop(store);
    if let Err(error) = drive(path, &mut run, clients, c, deadline, true, result) {
        run.state = run.durable_state.clone();
        if run.state.phase != "terminal" {
            run.state.phase = "recovery_required".into();
        }
        respond(request, &run, "collect", result)?;
        return Err(error.after_effects());
    }
    drop(lease);
    respond(request, &run, "collect", result)
}
type Prepared = (String, String, String, String, Vec<Target>, Vec<Client>);
fn prepare(
    normalized: &Value,
    context: &OperatorContext,
    id: &str,
    c: &RunControl,
    deadline: Instant,
) -> Result<Prepared, Fault> {
    let inventory = context
        .fleet_inventory_path
        .as_deref()
        .ok_or(Fault::new("fleet_inventory_required"))?;
    let (inventory_sha256, inventory_revision, entries) = config::load(inventory, c, deadline)?;
    let mut selected = Vec::new();
    for id in normalized["targets"].as_array().unwrap() {
        selected.push(
            entries
                .iter()
                .find(|e| id.as_str() == Some(e.id.as_str()))
                .cloned()
                .ok_or(Fault::new("invalid_fleet_selection"))?,
        );
    }
    let mut ids: Vec<_> = selected.iter().map(|e| e.id.clone()).collect();
    ids.sort();
    let selection_sha256 =
        model::digest(&json!({"inventory_sha256":inventory_sha256,"targets":ids}));
    let mut description = OperationResult::new(&Request::new("task.describe", json!({})));
    crate::tasks::describe(&json!({"id":"error-budget"}), &mut description);
    let binding = description.data["source"]["binding_digest"]
        .as_str()
        .unwrap()
        .to_string();
    let mut clients: Vec<Client> = Vec::new();
    let mut targets = Vec::new();
    for (index, entry) in selected.into_iter().enumerate() {
        let request_id = format!(
            "fleet-child-{}",
            &hash(format!("{id}:{index}").as_bytes())[7..]
        );
        match crate::runner::prepare_client(Path::new(&entry.runner_config), c, deadline) {
            Ok(client) if client.identity["runner_id"] == entry.runner_id => {
                let profile = Some(client.identity.clone());
                clients.push(Some(Arc::new(client)));
                targets.push(Target {
                    entry,
                    request_id,
                    profile,
                    policy_sha256: None,
                    max_runtime_ms: None,
                    preflight_error: None,
                });
            }
            _ => {
                clients.push(None);
                targets.push(Target {
                    entry,
                    request_id,
                    profile: None,
                    policy_sha256: None,
                    max_runtime_ms: None,
                    preflight_error: Some("fleet_profile_unavailable".into()),
                });
            }
        }
    }
    // All selected handshakes finish before any admission intent can be persisted/sent.
    for chunk in (0..targets.len())
        .collect::<Vec<_>>()
        .chunks(normalized["concurrency"].as_u64().unwrap() as usize)
    {
        let observations = std::thread::scope(|scope| {
            let mut workers = vec![];
            for &i in chunk {
                if let Some(client) = clients[i].as_ref() {
                    let client = client.clone();
                    let principal = targets[i].entry.principal.clone();
                    workers.push((
                        i,
                        scope.spawn(move || {
                            client.call(
                                "handshake",
                                json!({}),
                                &json!({"principal":principal}),
                                c,
                                deadline.min(Instant::now() + RPC),
                            )
                        }),
                    ));
                }
            }
            workers
                .into_iter()
                .map(|(i, h)| (i, h.join().ok()))
                .collect::<Vec<_>>()
        });
        for (i, receipt) in observations {
            let target = &mut targets[i];
            let Some(receipt) = receipt.filter(|r| r.execution.status == Status::Succeeded) else {
                target.preflight_error = Some("fleet_handshake_failed".into());
                continue;
            };
            let matching = receipt.data["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|cap| {
                    cap["target"] == target.entry.target
                        && cap["operation"] == "task.run"
                        && cap["version"] == 1
                        && cap["implementation_sha256"] == binding
                })
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                target.preflight_error = Some("fleet_task_grant_unavailable".into());
                continue;
            }
            target.policy_sha256 = receipt.data["runner"]["policy_sha256"]
                .as_str()
                .map(Into::into);
            target.max_runtime_ms = matching[0]["max_runtime_ms"].as_u64();
        }
    }
    let mut identities = BTreeSet::new();
    if targets.iter().filter(|t| t.profile.is_some()).any(|t| {
        !identities.insert(format!(
            "{}:{}:{}:{}",
            t.entry.runner_id,
            t.profile.as_ref().unwrap()["server_certificate_sha256"],
            t.entry.principal,
            t.entry.target
        ))
    }) {
        return Err(Fault::new("invalid_fleet_selection"));
    }
    Ok((
        inventory_sha256,
        inventory_revision,
        selection_sha256,
        binding,
        targets,
        clients,
    ))
}
fn prepared_binding(inventory: &str, selection: &str, binding: &str, targets: &[Target]) -> Value {
    json!({"inventory_sha256":inventory,"selection_sha256":selection,"task_binding_sha256":binding,"targets":targets.iter().map(|t|json!({"entry":t.entry,"profile":t.profile,"policy_sha256":t.policy_sha256,"max_runtime_ms":t.max_runtime_ms,"preflight_error":t.preflight_error})).collect::<Vec<_>>()})
}
pub(super) fn freeze(
    input: &Value,
    context: &OperatorContext,
    c: &RunControl,
    d: Instant,
) -> Result<(Value, bool), Fault> {
    let request = Request::new("fleet.collect", input.clone());
    let normalized = config::input(&request)?;
    let (inventory, _, selection, binding, targets, _) =
        prepare(&normalized, context, "freeze", c, d)?;
    if targets.iter().any(|t| t.preflight_error.is_some()) {
        return Err(Fault::new("fleet_binding_unavailable"));
    }
    let store = Store::open(path(context)?, true, c, d)?;
    Ok((
        json!({"store":store.identity()?,"selection":prepared_binding(&inventory,&selection,&binding,&targets)}),
        store.created,
    ))
}
fn reload(a: &Admission, c: &RunControl, d: Instant) -> Vec<Client> {
    a.targets
        .iter()
        .map(|t| {
            crate::runner::prepare_client(Path::new(&t.entry.runner_config), c, d)
                .ok()
                .filter(|p| Some(&p.identity) == t.profile.as_ref())
                .map(|p| Arc::new(p) as Arc<dyn Peer>)
        })
        .collect()
}
fn transaction(path: &Path, run: &mut Run, d: Instant) -> Result<(), Fault> {
    let c = RunControl::default();
    let store = Store::open(path, false, &c, d)?;
    let current = store.load(&run.admission.id)?;
    if current.admission.request_sha256 != run.admission.request_sha256
        || current.state.phase == "terminal"
    {
        return Err(Fault::new("fleet_ownership_changed"));
    }
    run.state.cancel_requested |= current.state.cancel_requested;
    run.state.updated_at = crate::result::timestamp();
    store.commit(run)
}
fn admit_intent(
    path: &Path,
    run: &mut Run,
    index: usize,
    c: &RunControl,
    deadline: Instant,
) -> Result<bool, Fault> {
    let durable = RunControl::default();
    let store = Store::open(path, false, &durable, deadline)?;
    let current = store.load(&run.admission.id)?;
    if current.admission.request_sha256 != run.admission.request_sha256
        || current.state.phase == "terminal"
        || current.state.targets[index].phase != "queued"
    {
        return Err(Fault::new("fleet_ownership_changed"));
    }
    run.state.cancel_requested |=
        current.state.cancel_requested || c.signal.load(Ordering::Relaxed) != 0;
    if run.state.cancel_requested || Instant::now() >= deadline {
        run.state.targets[index] = current.state.targets[index].clone();
        return Ok(false);
    }
    run.state.updated_at = crate::result::timestamp();
    store.commit(run)?;
    Ok(true)
}
fn finalize(path: &Path, run: &mut Run, d: Instant) -> Result<(), Fault> {
    let c = RunControl::default();
    let store = Store::open(path, false, &c, d)?;
    let current = store.load(&run.admission.id)?;
    run.state.cancel_requested |= current.state.cancel_requested;
    for i in 0..run.state.targets.len() {
        if run.state.targets[i].phase == "finalizing" {
            store.finalize_target(run, i)?;
        }
    }
    Ok(())
}
fn observe(
    a: &Admission,
    index: usize,
    s: &mut model::TargetState,
    method: &str,
    receipt: OperationResult,
) {
    let value = serde_json::to_value(&receipt).unwrap();
    let bytes = serde_json::to_vec(&value).unwrap();
    let error = receipt.errors.first().map(|e| e.code.clone());
    let data = &receipt.data;
    let previous = s.observation.as_ref();
    let observation = Observation {
        at: crate::result::timestamp(),
        admission: data["admission"].as_str().unwrap_or("unknown").into(),
        remote_state: data["job"]["state"].as_str().map(Into::into),
        job_id: data["job"]["job_id"]
            .as_str()
            .map(Into::into)
            .or_else(|| previous.and_then(|o| o.job_id.clone())),
        attempt_id: data["job"]["attempt_id"]
            .as_str()
            .map(Into::into)
            .or_else(|| previous.and_then(|o| o.attempt_id.clone())),
        result_sha256: data["result_sha256"]
            .as_str()
            .map(Into::into)
            .or_else(|| previous.and_then(|o| o.result_sha256.clone())),
        receipt: if bytes.len() <= model::MAX_RECEIPT {
            Some(value.clone())
        } else {
            None
        },
        receipt_sha256: hash(&bytes),
        receipt_omitted: bytes.len() > model::MAX_RECEIPT,
        error: error.clone(),
    };
    s.observation = Some(observation);
    s.outcome = Some("unknown".into());
    s.reason = error.or(Some("fleet_child_unresolved".into()));
    if method == "submit"
        && data["admission"] == "not_requested"
        && receipt.effect_outcome == "not_attempted"
    {
        s.phase = "finalizing".into();
        s.outcome = Some("failed".into());
        return;
    }
    if !data["job"].is_null() {
        s.phase = "admitted".into();
        if matches!(
            receipt.execution.status,
            Status::Succeeded | Status::Partial
        ) && matches!(
            data["job"]["state"].as_str(),
            Some("queued" | "running" | "cancelling" | "finalizing")
        ) {
            s.outcome = None;
            s.reason = None;
        }
    }
    let child = &data["result"];
    if data["job"]["state"] != "terminal" || !child.is_object() {
        return;
    }
    let target = &a.targets[index];
    let binding = &data["binding"];
    if binding["scope"]["principal"] != target.entry.principal
        || binding["scope"]["runner_id"] != target.entry.runner_id
        || binding["scope"]["target"]["id"] != target.entry.target
        || binding["scope"]["policy_sha256"].as_str() != target.policy_sha256.as_deref()
        || binding["implementation"]["source"]["binding_digest"] != a.task_binding_sha256
        || child["operation"] != "task.run"
        || child["request_id"] != target.request_id
    {
        s.reason = Some("fleet_child_binding_changed".into());
        return;
    }
    if child["execution"]["status"] == "unknown" || child["effect_outcome"] == "unknown" {
        return;
    }
    let success = child["execution"]["status"] == "succeeded"
        && child["data"]["source"]["binding_digest"] == a.task_binding_sha256
        && child["data"]["input"] == a.task["input"]
        && crate::runner::valid_budget_data(&child["data"]);
    s.phase = "finalizing".into();
    s.outcome = Some(if success { "succeeded" } else { "failed" }.into());
    s.reason = if success {
        None
    } else {
        Some("fleet_child_failed".into())
    };
}
fn drive(
    path: &Path,
    run: &mut Run,
    clients: Vec<Client>,
    c: &RunControl,
    deadline: Instant,
    admit: bool,
    result: &mut OperationResult,
) -> Result<(), Fault> {
    let mut stop = if admit {
        None
    } else {
        Some("interrupted".to_string())
    };
    let mut cleanup_deadline = None;
    if !admit {
        for target in &mut run.state.targets {
            if target.phase == "queued" {
                target.phase = "skipped".into();
                target.outcome = Some("skipped".into());
                target.reason = Some("fleet_parent_interrupted".into());
            }
        }
        run.state.phase = "active".into();
        transaction(path, run, deadline)?;
    }
    finalize(path, run, deadline + CLEANUP)?;
    let mut order: Vec<usize> = (0..run.admission.targets.len()).collect();
    order.sort_by_key(|i| run.admission.targets[*i].entry.id.clone());
    let offset = run.admission.ordinal as usize % order.len();
    order.rotate_left(offset);
    let cleanup_control = RunControl::default();
    std::thread::scope(|scope| -> Result<(), Fault> {
        let (sender, receiver) =
            std::sync::mpsc::channel::<(usize, &'static str, OperationResult)>();
        let mut workers: Vec<(usize, std::thread::ScopedJoinHandle<'_, ()>)> = vec![];
        let mut busy = BTreeSet::new();
        let mut authority_refused = BTreeSet::new();
        let mut cancel_sent = BTreeSet::new();
        let mut polled = vec![Instant::now() - Duration::from_secs(1); order.len()];
        let mut persistence_error = None;
        loop {
            let mut changed = false;
            while let Ok((index, method, receipt)) = receiver.try_recv() {
                busy.remove(&index);
                if receipt.data["remote_error"] == "runner_precondition_failed"
                    || receipt
                        .errors
                        .iter()
                        .any(|error| error.code == "runner_precondition_failed")
                {
                    // This call cannot restore the original authority. Retain the
                    // refusal instead of overwriting it with later timeout noise;
                    // a new explicit recovery call can check the same identity again.
                    authority_refused.insert(index);
                }
                observe(
                    &run.admission,
                    index,
                    &mut run.state.targets[index],
                    method,
                    receipt,
                );
                changed = true;
            }
            let mut k = 0;
            while k < workers.len() {
                if workers[k].1.is_finished() {
                    let (index, worker) = workers.swap_remove(k);
                    if worker.join().is_err() {
                        busy.remove(&index);
                        run.state.targets[index].reason = Some("fleet_observer_interrupted".into());
                        changed = true;
                    }
                } else {
                    k += 1
                }
            }
            if changed
                && let Err(e) =
                    transaction(path, run, cleanup_deadline.unwrap_or(deadline + CLEANUP)).and_then(
                        |_| finalize(path, run, cleanup_deadline.unwrap_or(deadline + CLEANUP)),
                    )
            {
                persistence_error = Some(e);
                break;
            }
            if run
                .state
                .targets
                .iter()
                .all(|s| matches!(s.phase.as_str(), "terminal" | "skipped"))
                && busy.is_empty()
            {
                run.state.phase = "terminal".into();
                run.state.stop_reason = stop.clone();
                transaction(path, run, cleanup_deadline.unwrap_or(deadline + CLEANUP))?;
                result.effect_outcome = "succeeded".into();
                break;
            }
            let durable = RunControl::default();
            match Store::open(
                path,
                false,
                &durable,
                Instant::now() + Duration::from_millis(100),
            )
            .and_then(|store| store.load(&run.admission.id))
            {
                Ok(current) => run.state.cancel_requested |= current.state.cancel_requested,
                Err(e) if e.code == "fleet_deadline_exceeded" => {}
                Err(e) => {
                    persistence_error = Some(e);
                    break;
                }
            }
            let counts = run.state.counts();
            if c.signal.load(Ordering::Relaxed) != 0 || run.state.cancel_requested {
                stop = Some("cancelled".into());
            } else if Instant::now() >= deadline {
                stop = Some("deadline".into());
            } else if admit
                && counts["failed"].as_u64().unwrap() + counts["unknown"].as_u64().unwrap()
                    >= run.admission.failure_threshold as u64
            {
                stop = Some("failure_threshold".into());
            }
            let stopping = stop.as_deref().is_some_and(|s| s != "interrupted");
            if stopping && cleanup_deadline.is_none() {
                cleanup_deadline = Some((Instant::now() + CLEANUP).min(deadline + CLEANUP));
                for target in &mut run.state.targets {
                    if target.phase == "queued" {
                        target.phase = "skipped".into();
                        target.outcome = Some("skipped".into());
                        target.reason = Some(format!("fleet_{}", stop.as_ref().unwrap()));
                    }
                }
                run.state.stop_reason = stop.clone();
                if let Err(e) = transaction(path, run, cleanup_deadline.unwrap()) {
                    persistence_error = Some(e);
                    break;
                }
            }
            let end = cleanup_deadline
                .map(|d| d - Duration::from_millis(250))
                .unwrap_or(deadline);
            if Instant::now() >= end {
                break;
            }
            let mut unresolved = run
                .state
                .targets
                .iter()
                .filter(|s| matches!(s.phase.as_str(), "intent" | "admitted"))
                .count();
            for &index in &order {
                if busy.len() >= run.admission.concurrency {
                    break;
                }
                if busy.contains(&index)
                    || authority_refused.contains(&index)
                    || matches!(
                        run.state.targets[index].phase.as_str(),
                        "terminal" | "finalizing" | "skipped"
                    )
                {
                    continue;
                }
                let Some(client) = clients[index].as_ref() else {
                    if run.state.targets[index].submit.is_some() {
                        run.state.targets[index].reason = Some("fleet_profile_changed".into());
                        run.state.targets[index].outcome = Some("unknown".into());
                    }
                    continue;
                };
                let target = run.admission.targets[index].clone();
                let (method, inputs, expected) = if run.state.targets[index].phase == "queued" {
                    if !admit || stopping || unresolved >= run.admission.concurrency {
                        continue;
                    }
                    let remaining = deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis() as u64;
                    if remaining == 0 {
                        continue;
                    }
                    let runtime = remaining.min(target.max_runtime_ms.unwrap());
                    let input = json!({"request_id":target.request_id,"target":target.entry.target,"operation":"task.run","operation_version":1,"inputs":run.admission.task,"max_runtime_ms":runtime,"retain_events":32});
                    run.state.targets[index].submit = Some(input.clone());
                    run.state.targets[index].phase = "intent".into();
                    run.state.targets[index].outcome = None;
                    run.state.targets[index].reason = Some("fleet_admission_intent".into());
                    match admit_intent(path, run, index, c, deadline) {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(e) => {
                            persistence_error = Some(e);
                            break;
                        }
                    }
                    unresolved += 1;
                    (
                        "submit",
                        input,
                        json!({"principal":run.admission.targets[index].entry.principal,"policy_sha256":run.admission.targets[index].policy_sha256}),
                    )
                } else {
                    if polled[index].elapsed() < Duration::from_millis(50) {
                        continue;
                    }
                    let method = if stopping && !cancel_sent.contains(&index) {
                        cancel_sent.insert(index);
                        "cancel"
                    } else {
                        "status"
                    };
                    (
                        method,
                        json!({"request_id":target.request_id}),
                        json!({"principal":target.entry.principal}),
                    )
                };
                polled[index] = Instant::now();
                busy.insert(index);
                let client = client.clone();
                let sender = sender.clone();
                let ctl = if stopping { &cleanup_control } else { c };
                let rpc_deadline = end.min(Instant::now() + RPC);
                workers.push((
                    index,
                    scope.spawn(move || {
                        let receipt = client.call(method, inputs, &expected, ctl, rpc_deadline);
                        let _ = sender.send((index, method, receipt));
                    }),
                ));
            }
            if persistence_error.is_some() {
                break;
            }
            if busy.is_empty()
                && run.state.targets.iter().enumerate().all(|(i, s)| {
                    matches!(s.phase.as_str(), "terminal" | "skipped")
                        || clients[i].is_none()
                        || authority_refused.contains(&i)
                })
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        for (_, worker) in workers {
            let _ = worker.join();
        }
        if let Some(e) = persistence_error {
            return Err(e.after_effects());
        }
        while let Ok((index, method, receipt)) = receiver.try_recv() {
            observe(
                &run.admission,
                index,
                &mut run.state.targets[index],
                method,
                receipt,
            );
        }
        if run.state.phase != "terminal" {
            run.state.phase = "recovery_required".into();
            run.state.stop_reason = stop;
            transaction(path, run, cleanup_deadline.unwrap_or(deadline + CLEANUP))?;
            finalize(path, run, cleanup_deadline.unwrap_or(deadline + CLEANUP))?;
            if run
                .state
                .targets
                .iter()
                .all(|s| matches!(s.phase.as_str(), "terminal" | "skipped"))
            {
                run.state.phase = "terminal".into();
                transaction(path, run, cleanup_deadline.unwrap_or(deadline + CLEANUP))?;
            }
        }
        Ok(())
    })
}
fn respond(
    request: &Request,
    run: &Run,
    action: &str,
    result: &mut OperationResult,
) -> Result<(), Fault> {
    let a = &run.admission;
    let mut phase = run.state.phase.clone();
    if phase == "active" && store::claim(run)?.is_some() {
        phase = "recovery_required".into();
    }
    let aggregate = run.state.aggregate();
    let counts = run.state.counts();
    let unknown = counts["unknown"].as_u64().unwrap() > 0
        || counts["pending"].as_u64().unwrap() > 0
        || run
            .state
            .targets
            .iter()
            .filter_map(|t| t.observation.as_ref())
            .filter_map(|o| o.receipt.as_ref())
            .any(|r| r["data"]["result"]["effect_outcome"] == "unknown");
    let targets:Vec<Value>=a.targets.iter().zip(&run.state.targets).enumerate().map(|(i,(t,s))|{let o=s.observation.as_ref();json!({"id":t.entry.id,"runner_id":t.entry.runner_id,"target":t.entry.target,"principal":t.entry.principal,"request_id":t.request_id,"phase":s.phase,"outcome":s.outcome,"reason":s.reason,"job_id":o.and_then(|o|o.job_id.as_ref()),"attempt_id":o.and_then(|o|o.attempt_id.as_ref()),"result_sha256":o.and_then(|o|o.result_sha256.as_ref()),"evidence_ref":s.evidence_sha256.as_ref().filter(|_|s.phase=="terminal").map(|sha|format!("fleet-run:{}/target-{i:02}.json@{sha}",a.id)),"receipt":o.and_then(|o|o.receipt.as_ref()),"receipt_omitted":o.is_some_and(|o|o.receipt_omitted)})}).collect();
    result.data = json!({"format_version":1,"action":action,"run":{"id":a.id,"request_id":a.request_id,"owner_uid":a.owner_uid,"inventory_sha256":a.inventory_sha256,"inventory_revision":a.inventory_revision,"selection_sha256":a.selection_sha256,"task_binding_sha256":a.task_binding_sha256,"state":phase,"created_at":a.created_at,"expires_at":a.expires_at,"timeout_ms":a.timeout_ms,"concurrency":a.concurrency,"failure_threshold":a.failure_threshold,"ordinal":a.ordinal,"cancel_requested":run.state.cancel_requested,"stop_reason":run.state.stop_reason},"counts":counts,"targets":targets,"aggregate":aggregate,"aggregate_effect_outcome":if unknown{"unknown"}else{"not_applicable"},"cancellation_signal":null});
    let rows = result.data["targets"].as_array_mut().unwrap();
    let mut used = 0;
    let mut omitted = false;
    for row in rows {
        let size = serde_json::to_vec(&row["receipt"]).unwrap().len();
        if used + size > result.effective_limits.max_output_bytes && !row["receipt"].is_null() {
            row["receipt"] = Value::Null;
            row["receipt_omitted"] = json!(true);
            omitted = true;
        } else {
            used += size;
        }
    }
    if matches!(request.operation.as_str(), "fleet.collect" | "fleet.resume") {
        result.execution.status = match aggregate {
            "succeeded" => {
                if omitted {
                    Status::Partial
                } else {
                    Status::Succeeded
                }
            }
            "partial" => Status::Partial,
            "failed" => Status::Failed,
            _ => Status::Unknown,
        };
        if matches!(result.execution.status, Status::Failed | Status::Unknown) {
            result.error("fleet_collection_incomplete","The frozen collection has failed or unresolved targets; no successful service-health conclusion is established.");
        }
        if unknown {
            result.effect_outcome = "unknown".into();
        }
    } else if result.effect_outcome == "not_attempted" {
        result.effect_outcome = "not_applicable".into();
    }
    if omitted && result.execution.status == Status::Succeeded {
        result.execution.status = Status::Partial;
    }
    result.coverage.state = if aggregate == "succeeded" && !omitted {
        "complete"
    } else {
        "partial"
    }
    .into();
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    struct RefusedPeer(std::sync::Mutex<Vec<String>>);
    impl Peer for RefusedPeer {
        fn call(
            &self,
            method: &str,
            input: Value,
            _expected: &Value,
            _c: &RunControl,
            _d: Instant,
        ) -> OperationResult {
            self.0.lock().unwrap().push(method.into());
            let mut receipt =
                OperationResult::new(&Request::new(&format!("runner.{method}"), input));
            receipt.execution.status = Status::Denied;
            receipt.error("runner_remote_refusal", "Synthetic authenticated refusal.");
            receipt.data = json!({"admission":"not_requested","remote_error":"runner_precondition_failed","job":null,"result":null});
            receipt
        }
    }
    #[test]
    fn recovery_retains_authority_refusal_without_repeated_poll_or_cancel() {
        let path = std::env::temp_dir().join(crate::result::new_id("fleet-refusal-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(5);
        let store = Store::open(&path, true, &c, d).unwrap();
        let a = model::fixture();
        let mut run = store.create(a.clone()).unwrap();
        run.state.targets[0].phase = "intent".into();
        run.state.targets[0].outcome = Some("unknown".into());
        run.state.targets[0].submit = Some(
            json!({"request_id":a.targets[0].request_id,"target":a.targets[0].entry.target,"operation":"task.run","operation_version":1,"inputs":a.task,"max_runtime_ms":1000,"retain_events":32}),
        );
        store.commit(&mut run).unwrap();
        let lease = store::claim(&run).unwrap().unwrap();
        drop(store);
        let peer = Arc::new(RefusedPeer(std::sync::Mutex::new(vec![])));
        let mut result = OperationResult::new(&Request::new(
            "fleet.resume",
            json!({"request_id":"fixture-parent"}),
        ));
        drive(
            &path,
            &mut run,
            vec![Some(peer.clone())],
            &c,
            d,
            false,
            &mut result,
        )
        .unwrap();
        assert_eq!(*peer.0.lock().unwrap(), vec!["status"]);
        assert_eq!(run.state.phase, "recovery_required");
        assert_eq!(run.state.targets[0].outcome.as_deref(), Some("unknown"));
        let store = Store::open(&path, false, &c, d).unwrap();
        let stored = store.load(&run.admission.id).unwrap();
        let observation = stored.state.targets[0].observation.as_ref().unwrap();
        assert_eq!(
            observation.receipt.as_ref().unwrap()["data"]["remote_error"],
            "runner_precondition_failed"
        );
        drop(store);
        drop(lease);
        std::fs::remove_dir_all(path).unwrap();
    }
    struct Spy {
        calls: std::sync::Mutex<Vec<String>>,
        admission: Admission,
    }
    impl Peer for Spy {
        fn call(
            &self,
            method: &str,
            input: Value,
            _expected: &Value,
            _c: &RunControl,
            _d: Instant,
        ) -> OperationResult {
            self.calls.lock().unwrap().push(method.into());
            assert_ne!(
                method, "submit",
                "recovery must never call submit even when runner deduplicates"
            );
            let t = &self.admission.targets[0];
            assert_eq!(input["request_id"], t.request_id);
            let mut child_request = Request::new("task.run", self.admission.task.clone());
            child_request.request_id = t.request_id.clone();
            let mut child = OperationResult::new(&child_request);
            child.execution.status = Status::Failed;
            child.error(
                "fixture_failure",
                "Synthetic already-completed child failure.",
            );
            let mut receipt =
                OperationResult::new(&Request::new(&format!("runner.{method}"), input));
            receipt.effect_outcome = "not_applicable".into();
            receipt.data = json!({"admission":"accepted","job":{"state":"terminal","job_id":"known-job","attempt_id":null},"result":child,"result_sha256":format!("sha256:{}","b".repeat(64)),"binding":{"scope":{"principal":t.entry.principal,"runner_id":t.entry.runner_id,"target":{"id":t.entry.target},"policy_sha256":t.policy_sha256},"implementation":{"source":{"binding_digest":self.admission.task_binding_sha256}}}});
            receipt
        }
    }
    #[test]
    fn resumed_controller_queries_original_intent_and_never_submits_queued_targets() {
        let path = std::env::temp_dir().join(crate::result::new_id("fleet-resume-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(5);
        let store = Store::open(&path, true, &c, d).unwrap();
        let mut a = model::fixture();
        let mut other = a.targets[0].clone();
        other.entry.id = "two".into();
        other.request_id = "never-submitted".into();
        a.targets.push(other);
        a.request_sha256 = a.fingerprint();
        let mut run = store.create(a.clone()).unwrap();
        run.state.targets[0].phase = "intent".into();
        run.state.targets[0].outcome = Some("unknown".into());
        run.state.targets[0].submit = Some(
            json!({"request_id":a.targets[0].request_id,"target":a.targets[0].entry.target,"operation":"task.run","operation_version":1,"inputs":a.task,"max_runtime_ms":1000,"retain_events":32}),
        );
        store.commit(&mut run).unwrap();
        let lease = store::claim(&run).unwrap().unwrap();
        drop(store);
        let spy = Arc::new(Spy {
            calls: std::sync::Mutex::new(vec![]),
            admission: a,
        });
        let clients: Vec<Client> = vec![Some(spy.clone()), Some(spy.clone())];
        let mut result = OperationResult::new(&Request::new(
            "fleet.resume",
            json!({"request_id":"fixture-parent"}),
        ));
        drive(&path, &mut run, clients, &c, d, false, &mut result).unwrap();
        assert_eq!(*spy.calls.lock().unwrap(), vec!["status"]);
        assert_eq!(run.state.phase, "terminal");
        assert_eq!(run.state.targets[0].outcome.as_deref(), Some("failed"));
        assert_eq!(run.state.targets[1].outcome.as_deref(), Some("skipped"));
        drop(lease);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn owner_commit_merges_a_concurrent_durable_cancel_flag() {
        let path = std::env::temp_dir().join(crate::result::new_id("fleet-cancel-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(5);
        let store = Store::open(&path, true, &c, d).unwrap();
        let mut run = store.create(model::fixture()).unwrap();
        let mut other = store.load(&run.admission.id).unwrap();
        other.state.cancel_requested = true;
        store.commit(&mut other).unwrap();
        drop(store);
        transaction(&path, &mut run, d).unwrap();
        assert!(run.state.cancel_requested);
        let store = Store::open(&path, false, &c, d).unwrap();
        assert!(
            store
                .load(&run.admission.id)
                .unwrap()
                .state
                .cancel_requested
        );
        drop(store);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn cancellation_committed_before_intent_prevents_new_admission() {
        let path = std::env::temp_dir().join(crate::result::new_id("fleet-admission-race-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(5);
        let store = Store::open(&path, true, &c, d).unwrap();
        let mut owner = store.create(model::fixture()).unwrap();
        let mut cancel = store.load(&owner.admission.id).unwrap();
        cancel.state.cancel_requested = true;
        store.commit(&mut cancel).unwrap();
        drop(store);
        owner.state.targets[0].phase = "intent".into();
        owner.state.targets[0].submit = Some(
            json!({"request_id":owner.admission.targets[0].request_id,"target":owner.admission.targets[0].entry.target,"operation":"task.run","operation_version":1,"inputs":owner.admission.task,"max_runtime_ms":1000,"retain_events":32}),
        );
        assert!(!admit_intent(&path, &mut owner, 0, &c, d).unwrap());
        assert!(owner.state.cancel_requested);
        assert_eq!(owner.state.targets[0].phase, "queued");
        assert!(owner.state.targets[0].submit.is_none());
        let store = Store::open(&path, false, &c, d).unwrap();
        assert_eq!(
            store.load(&owner.admission.id).unwrap().state.targets[0].phase,
            "queued"
        );
        drop(store);
        std::fs::remove_dir_all(path).unwrap();
    }
}
