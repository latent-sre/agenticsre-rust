use super::*;
use model::{Generation, Occurrence, Schedule, State};
use std::path::Path;
use store::Store;
const RESERVE_MS: u64 = 3500;
fn path(context: &OperatorContext) -> Result<&Path, Fault> {
    context
        .schedule_state_dir
        .as_deref()
        .ok_or(Fault::new("schedule_store_required"))
}
fn now() -> String {
    crate::result::timestamp()
}
fn revision(s: &Schedule, input: &Value) -> Result<(), Fault> {
    if input["expected_revision"] != s.revision {
        Err(Fault::new("schedule_revision_conflict"))
    } else {
        Ok(())
    }
}
fn index(state: &State, id: &str) -> Result<usize, Fault> {
    state
        .schedules
        .iter()
        .position(|s| s.id == id)
        .ok_or(Fault::new("schedule_not_found"))
}
fn advance(s: &mut Schedule) -> Result<(), Fault> {
    s.revision = s
        .revision
        .checked_add(1)
        .filter(|n| *n < model::MAX_SAFE)
        .ok_or(Fault::new("schedule_quota_exceeded"))?;
    s.updated_at = now();
    Ok(())
}
fn generation(
    definition: Value,
    number: u64,
    context: &OperatorContext,
    c: &RunControl,
    d: Instant,
    result: &mut OperationResult,
) -> Result<Generation, Fault> {
    let fleet_store = context
        .fleet_state_dir
        .as_ref()
        .and_then(|p| p.to_str())
        .ok_or(Fault::new("fleet_store_required"))?
        .to_owned();
    let inventory = context
        .fleet_inventory_path
        .as_ref()
        .and_then(|p| p.to_str())
        .ok_or(Fault::new("fleet_inventory_required"))?
        .to_owned();
    let (binding, effects) = crate::fleet::freeze(
        &config::fleet_input(&definition, "schedule-freeze"),
        context,
        c,
        d,
    )
    .map_err(|(code, effects)| Fault { code, effects })?;
    if effects {
        result.effect_outcome = "succeeded".into();
    }
    let mut g = Generation {
        number,
        definition,
        tzdb_version: model::TZDB.into(),
        binding,
        binding_sha256: String::new(),
        fleet_store,
        inventory,
    };
    g.binding_sha256 = g.fingerprint();
    Ok(g)
}
pub(super) fn run(
    r: &Request,
    c: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
    worker: bool,
) -> Result<(), Fault> {
    let input = config::input(r)?;
    let deadline = start + Duration::from_millis(r.limits.timeout_ms);
    active(c, deadline)?;
    if r.operation == "schedule.work" {
        if !worker {
            return Err(Fault::new("schedule_worker_entry_required"));
        }
        return work(r, c, context, deadline, result);
    }
    let storepath = path(context)?;
    if matches!(r.operation.as_str(), "schedule.create" | "schedule.update") {
        let id = input["id"].as_str().unwrap();
        match Store::open(storepath, false, c, deadline) {
            Ok(store) => {
                let state = store.load()?;
                if r.operation == "schedule.create" {
                    if state.schedules.iter().any(|s| s.id == id) {
                        return Err(Fault::new("schedule_exists"));
                    }
                    if state.schedules.len() >= 16 {
                        return Err(Fault::new("schedule_quota_exceeded"));
                    }
                } else {
                    let s = &state.schedules[index(&state, id)?];
                    revision(s, &input)?;
                    if s.generations.len() >= 128
                        || s.occurrences.len()
                            > input["definition"]["retention_limit"].as_u64().unwrap() as usize
                    {
                        return Err(Fault::new("schedule_quota_exceeded"));
                    }
                }
            }
            Err(e) if e.code == "schedule_store_not_found" && r.operation == "schedule.create" => {}
            Err(e) => return Err(e),
        }
        let number = if r.operation == "schedule.create" {
            1
        } else {
            let store = Store::open(storepath, false, c, deadline)?;
            let state = store.load()?;
            state.schedules[index(&state, id)?].generation + 1
        };
        let g = generation(
            input["definition"].clone(),
            number,
            context,
            c,
            deadline,
            result,
        )?;
        let cursor = calendar::starting_cursor(
            &g.definition["cadence"],
            g.definition["timezone"].as_str().unwrap(),
            time::OffsetDateTime::now_utc(),
        )
        .map_err(Fault::new)?;
        let store = Store::open(storepath, r.operation == "schedule.create", c, deadline)?;
        if store.created {
            result.effect_outcome = "succeeded".into();
        }
        let mut state = store.load()?;
        let action = if r.operation == "schedule.create" {
            if state.schedules.iter().any(|s| s.id == id) {
                return Err(Fault::new("schedule_exists"));
            }
            if state.schedules.len() >= 16 {
                return Err(Fault::new("schedule_quota_exceeded"));
            }
            let at = now();
            state.schedules.push(Schedule {
                instance_id: crate::result::new_id("schedule"),
                id: id.into(),
                owner_uid: model::uid(),
                revision: 1,
                generation: 1,
                enabled: g.definition["enabled"].as_bool().unwrap(),
                created_at: at.clone(),
                updated_at: at,
                generations: vec![g],
                cursor,
                last_skip: None,
                occurrences: vec![],
            });
            "created"
        } else {
            let i = index(&state, id)?;
            let s = &mut state.schedules[i];
            revision(s, &input)?;
            if s.generation + 1 != number {
                return Err(Fault::new("schedule_revision_conflict"));
            }
            advance(s)?;
            s.generation = number;
            s.enabled = g.definition["enabled"].as_bool().unwrap();
            s.generations.push(g);
            s.cursor = cursor;
            for o in &mut s.occurrences {
                if o.state == "queued" {
                    o.state = "skipped".into();
                    o.reason = Some("generation_changed".into());
                    o.updated_at = now();
                }
            }
            "updated"
        };
        store.commit(&state, c, deadline)?;
        result.effect_outcome = "succeeded".into();
        return respond(r, &state, Some(id), action, result);
    }
    let store = Store::open(storepath, false, c, deadline)?;
    let mut state = store.load()?;
    if r.operation == "schedule.list" {
        return respond(r, &state, None, "list", result);
    }
    let id = input["id"].as_str().unwrap();
    let i = index(&state, id)?;
    if r.operation == "schedule.status" {
        return respond(r, &state, Some(id), "status", result);
    }
    if r.operation == "schedule.preview" {
        let s = &state.schedules[i];
        let g = s.current();
        if g.tzdb_version != model::TZDB {
            return Err(Fault::new("schedule_timezone_rules_changed"));
        }
        let preview = calendar::preview(
            &g.definition["cadence"],
            g.definition["timezone"].as_str().unwrap(),
            s.cursor,
            input["count"].as_u64().unwrap() as usize,
        )
        .map_err(Fault::new)?;
        respond(r, &state, Some(id), "preview", result)?;
        result.data["preview"] = serde_json::to_value(preview).unwrap();
        return Ok(());
    }
    // Manual dedup precedes CAS: an old request cannot rebind itself after edits or pause.
    if r.operation == "schedule.run-now"
        && let Some(o) = state.schedules[i]
            .occurrences
            .iter()
            .find(|o| o.manual_request_id.as_deref() == input["request_id"].as_str())
    {
        if o.request_sha256.as_ref() != Some(&hash(&input)) {
            return Err(Fault::new("schedule_request_conflict"));
        }
        return respond(r, &state, Some(id), "existing", result);
    }
    let s = &mut state.schedules[i];
    revision(s, &input)?;
    let action = match r.operation.as_str() {
        "schedule.pause" => {
            s.enabled = false;
            "paused"
        }
        "schedule.cancel-active" => {
            for o in &mut s.occurrences {
                if o.unresolved() {
                    o.cancel_requested = true;
                    o.updated_at = now();
                }
            }
            "cancel_requested"
        }
        "schedule.run-now" => {
            if s.occurrences.len()
                >= s.current().definition["retention_limit"].as_u64().unwrap() as usize
            {
                return Err(Fault::new("schedule_quota_exceeded"));
            }
            let mut o = occurrence(s, "manual", None, now(), None);
            o.manual_request_id = input["request_id"].as_str().map(Into::into);
            o.request_sha256 = Some(hash(&input));
            s.occurrences.push(o);
            "queued"
        }
        _ => return Err(Fault::new("invalid_schedule_input")),
    };
    advance(s)?;
    store.commit(&state, c, deadline)?;
    result.effect_outcome = "succeeded".into();
    respond(r, &state, Some(id), action, result)?;
    if r.operation == "schedule.cancel-active" {
        let pending: Vec<_> = state.schedules[i]
            .occurrences
            .iter()
            .filter(|o| o.unresolved())
            .map(|o| {
                (
                    o.clone(),
                    state.schedules[i]
                        .generations
                        .iter()
                        .find(|g| g.number == o.generation)
                        .unwrap()
                        .clone(),
                )
            })
            .collect();
        drop(store);
        for (o, g) in pending {
            let receipt = call(&o, &g, false, context, c, deadline);
            let durable = RunControl::default();
            let finish = Instant::now() + Duration::from_millis(1000);
            let store = Store::open(storepath, false, &durable, finish)?;
            let mut state = store.load()?;
            let si = index(&state, id)?;
            if let Some(current) = state.schedules[si]
                .occurrences
                .iter_mut()
                .find(|n| n.id == o.id && n.unresolved())
            {
                observe(current, receipt, false);
                store.commit(&state, &durable, finish)?;
            }
            respond(r, &state, Some(id), action, result)?;
        }
        return Ok(());
    }
    Ok(())
}
fn occurrence(
    s: &Schedule,
    trigger: &str,
    ordinal: Option<i64>,
    due_at: String,
    local: Option<String>,
) -> Occurrence {
    let id = if let Some(n) = ordinal {
        format!(
            "schedule-occurrence-{}",
            &hash(&json!([s.instance_id, s.id, s.generation, n]))[7..]
        )
    } else {
        crate::result::new_id("schedule-occurrence")
    };
    let fleet_request_id = format!("schedule-fleet-{}", &hash(&json!(id))[7..]);
    let at = now();
    Occurrence {
        id,
        last_work_sequence: 0,
        schedule_id: s.id.clone(),
        generation: s.generation,
        trigger: trigger.into(),
        manual_request_id: None,
        request_sha256: None,
        ordinal,
        due_at,
        local,
        state: "queued".into(),
        reason: None,
        fleet_request_id,
        effective_timeout_ms: None,
        fleet_run_id: None,
        fleet_aggregate: None,
        fleet_effect_outcome: None,
        receipt_sha256: None,
        observation_sha256: None,
        receipt: None,
        receipt_omitted: false,
        cancel_requested: false,
        created_at: at.clone(),
        updated_at: at,
    }
}
// Pure injected-clock planning; no public clock override. Cursor never rewinds.
fn plan(s: &mut Schedule, at: time::OffsetDateTime) -> Result<bool, Fault> {
    if !s.enabled {
        return Ok(false);
    }
    let g = s.current();
    if g.tzdb_version != model::TZDB {
        return Err(Fault::new("schedule_timezone_rules_changed"));
    }
    let batch = calendar::due(
        &g.definition["cadence"],
        g.definition["timezone"].as_str().unwrap(),
        s.cursor,
        at,
        g.definition["misfire_grace_ms"].as_u64().unwrap(),
    )
    .map_err(Fault::new)?;
    if batch.cursor <= s.cursor {
        return Ok(false);
    }
    if batch.latest.is_some()
        && s.occurrences.len() >= g.definition["retention_limit"].as_u64().unwrap() as usize
    {
        return Err(Fault::new("schedule_quota_exceeded"));
    }
    s.cursor = batch.cursor;
    if let Some(skipped) = batch.skipped {
        s.last_skip = Some(model::Skip {
            from_ordinal: skipped.from_ordinal,
            to_ordinal: skipped.to_ordinal,
            count: skipped.count,
            reason: skipped.reason,
            at: at
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            generation: s.generation,
        });
    }
    if let Some(due) = batch.latest {
        let mut o = occurrence(s, "due", Some(due.ordinal), due.due_utc, Some(due.local));
        if due.state == "nonexistent" {
            o.state = "skipped".into();
            o.reason = Some("nonexistent_local_time".into());
        } else if s.occurrences.iter().any(Occurrence::unresolved) {
            o.state = "skipped".into();
            o.reason = Some("overlap".into());
        }
        s.occurrences.push(o);
    }
    Ok(true)
}
// This function executes while the short store lock is held. Pause/edit/cancel can
// therefore linearize before intent; queued rows never give dispatch authority.
fn admit(
    s: &mut Schedule,
    index: usize,
    remaining_ms: u64,
    at: time::OffsetDateTime,
) -> Result<bool, Fault> {
    let o = &s.occurrences[index];
    if o.state != "queued"
        || o.generation != s.generation
        || (!s.enabled && o.trigger == "due")
        || s.occurrences.iter().any(Occurrence::unresolved)
    {
        return Ok(false);
    }
    if o.trigger == "due" {
        let due =
            time::OffsetDateTime::parse(&o.due_at, &time::format_description::well_known::Rfc3339)
                .map_err(|_| Fault::new("schedule_store_recovery_required"))?;
        if at < due {
            return Ok(false);
        }
        if (at - due).whole_nanoseconds()
            > i128::from(s.current().definition["misfire_grace_ms"].as_u64().unwrap()) * 1_000_000
        {
            let o = &mut s.occurrences[index];
            o.state = "skipped".into();
            o.reason = Some("misfire".into());
            o.updated_at = now();
            return Ok(false);
        }
    }
    if remaining_ms <= RESERVE_MS {
        return Ok(false);
    }
    if s.current().tzdb_version != model::TZDB {
        return Err(Fault::new("schedule_timezone_rules_changed"));
    }
    let budget = s.current().definition["collection"]["timeout_ms"]
        .as_u64()
        .unwrap()
        .min(remaining_ms - RESERVE_MS);
    let o = &mut s.occurrences[index];
    o.effective_timeout_ms = Some(budget);
    o.state = "intent".into();
    o.updated_at = now();
    Ok(true)
}
fn work(
    r: &Request,
    c: &RunControl,
    context: &OperatorContext,
    d: Instant,
    result: &mut OperationResult,
) -> Result<(), Fault> {
    let p = path(context)?;
    let store = Store::open(p, false, c, d)?;
    let _lease = store.claim()?;
    drop(store);
    let mut touched = std::collections::BTreeSet::new();
    let limit = r.inputs["max_occurrences"].as_u64().unwrap() as usize;
    let mut failed_this_pass = false;
    for _ in 0..limit {
        active(c, d)?;
        let store = Store::open(p, false, c, d)?;
        let mut state = store.load()?;
        let mut changed = false;
        for s in &mut state.schedules {
            changed |= plan(s, time::OffsetDateTime::now_utc())?;
        }
        let mut chosen = None;
        let mut candidates = vec![];
        for (si, s) in state.schedules.iter().enumerate() {
            for (oi, o) in s.occurrences.iter().enumerate() {
                if !touched.contains(&o.id) && (o.unresolved() || o.state == "queued") {
                    candidates.push((o.last_work_sequence, si, oi));
                }
            }
        }
        candidates.sort_unstable();
        for (_, si, oi) in candidates {
            let s = &mut state.schedules[si];
            let fresh = !s.occurrences[oi].unresolved();
            if fresh {
                let before = s.occurrences[oi].state.clone();
                let admitted = admit(
                    s,
                    oi,
                    d.saturating_duration_since(Instant::now()).as_millis() as u64,
                    time::OffsetDateTime::now_utc(),
                )?;
                changed |= before != s.occurrences[oi].state;
                if !admitted {
                    continue;
                }
            }
            state.work_sequence = state
                .work_sequence
                .checked_add(1)
                .filter(|n| *n < model::MAX_SAFE)
                .ok_or(Fault::new("schedule_quota_exceeded"))?;
            state.schedules[si].occurrences[oi].last_work_sequence = state.work_sequence;
            changed = true;
            chosen = Some((si, oi, fresh));
            break;
        }
        if changed {
            store.commit(&state, c, d)?;
            result.effect_outcome = "succeeded".into();
        }
        let Some((si, oi, fresh)) = chosen else {
            respond(r, &state, None, "worked", result)?;
            if failed_this_pass && result.execution.status != Status::Unknown {
                result.execution.status = Status::Failed;
            }
            return Ok(());
        };
        respond(r, &state, None, "worked", result)?;
        let o = state.schedules[si].occurrences[oi].clone();
        let g = state.schedules[si]
            .generations
            .iter()
            .find(|g| g.number == o.generation)
            .unwrap()
            .clone();
        touched.insert(o.id.clone());
        drop(store);
        let receipt = call(&o, &g, fresh, context, c, d);
        // A signal detaches; it never grants cancellation of admitted observations.
        let durable = RunControl::default();
        let commit_deadline = Instant::now() + Duration::from_millis(1000);
        let store = Store::open(p, false, &durable, commit_deadline)?;
        let mut state = store.load()?;
        let si = index(&state, &o.schedule_id)?;
        let oi = state.schedules[si]
            .occurrences
            .iter()
            .position(|n| n.id == o.id)
            .ok_or(Fault::new("schedule_store_recovery_required"))?;
        if !state.schedules[si].occurrences[oi].resolved() {
            observe(&mut state.schedules[si].occurrences[oi], receipt, fresh);
            store.commit(&state, &durable, commit_deadline)?;
        }
        result.effect_outcome = "succeeded".into();
        failed_this_pass |= state.schedules[si].occurrences[oi].state == "failed";
        respond(r, &state, None, "worked", result)?;
        if failed_this_pass && result.execution.status != Status::Unknown {
            result.execution.status = Status::Failed;
        }
        if c.signal.load(Ordering::Relaxed) != 0 {
            return Err(Fault::new("schedule_cancelled").after_effects());
        }
    }
    Ok(())
}
fn call(
    o: &Occurrence,
    g: &Generation,
    fresh: bool,
    context: &OperatorContext,
    c: &RunControl,
    d: Instant,
) -> Result<OperationResult, &'static str> {
    if context.fleet_state_dir.as_deref() != Some(Path::new(&g.fleet_store)) {
        return Err("fleet_namespace_changed");
    }
    let method = if fresh {
        "fleet.collect"
    } else if o.cancel_requested {
        "fleet.cancel"
    } else {
        "fleet.resume"
    };
    let input = if fresh {
        config::fleet_input(&g.definition, &o.fleet_request_id)
    } else {
        json!({"request_id":o.fleet_request_id})
    };
    let mut request = Request::new(method, input);
    request.record = "always".into();
    request.limits.timeout_ms = if fresh {
        o.effective_timeout_ms.unwrap()
    } else {
        d.saturating_duration_since(Instant::now())
            .as_millis()
            .clamp(1, 10000) as u64
    };
    request.limits.max_output_bytes = 16384;
    let rx = launch(request, g);
    let mut cancel_rx: Option<std::sync::mpsc::Receiver<OperationResult>> = None;
    let mut cancel_forwarded = o.cancel_requested;
    let mut check_at = Instant::now();
    loop {
        if let Some(receiver) = &cancel_rx {
            match receiver.try_recv() {
                Ok(receipt) => {
                    cancel_forwarded = receipt.data["run"]["cancel_requested"] == true
                        || receipt.data["run"]["state"] == "terminal";
                    cancel_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    cancel_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if !cancel_forwarded
            && cancel_rx.is_none()
            && Instant::now() >= check_at
            && Instant::now() < d
        {
            check_at = Instant::now() + Duration::from_millis(250);
            if cancellation_requested(context, o, c, d) {
                let mut request =
                    Request::new("fleet.cancel", json!({"request_id":o.fleet_request_id}));
                request.record = "always".into();
                request.limits.timeout_ms = d
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .clamp(1, 1000) as u64;
                request.limits.max_output_bytes = 1024;
                cancel_rx = Some(launch(request, g));
            }
        }
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(receipt) => return Ok(receipt),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("schedule_fleet_worker_lost");
            }
            Err(_) => {
                if c.signal.load(Ordering::Relaxed) != 0 {
                    return Err("schedule_worker_interrupted");
                }
                if Instant::now() >= d {
                    return Err("schedule_deadline_exceeded");
                }
            }
        }
    }
}
fn cancellation_requested(
    context: &OperatorContext,
    o: &Occurrence,
    c: &RunControl,
    d: Instant,
) -> bool {
    let Some(path) = context.schedule_state_dir.as_deref() else {
        return false;
    };
    Store::open(
        path,
        false,
        c,
        d.min(Instant::now() + Duration::from_millis(25)),
    )
    .and_then(|s| s.load())
    .ok()
    .is_some_and(|state| {
        state
            .schedules
            .iter()
            .find(|s| s.id == o.schedule_id)
            .is_some_and(|s| {
                s.occurrences
                    .iter()
                    .any(|n| n.id == o.id && n.cancel_requested)
            })
    })
}
fn launch(request: Request, g: &Generation) -> std::sync::mpsc::Receiver<OperationResult> {
    let local = OperatorContext {
        fleet_state_dir: Some(g.fleet_store.clone().into()),
        fleet_inventory_path: Some(g.inventory.clone().into()),
        ..OperatorContext::default()
    };
    let binding = g.binding.clone();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let control = RunControl::default();
        let start = Instant::now();
        let mut receipt = OperationResult::new(&request);
        crate::fleet::run_expected(
            &request,
            &control,
            &local,
            start,
            &mut receipt,
            Some(&binding),
        );
        receipt.finish(start);
        let _ = tx.send(receipt);
    });
    rx
}
fn observe(o: &mut Occurrence, receipt: Result<OperationResult, &'static str>, fresh: bool) {
    o.updated_at = now();
    match receipt {
        Err(code) => {
            o.state = "unknown".into();
            o.reason = Some(code.into());
        }
        Ok(receipt) => {
            let v = serde_json::to_value(receipt).unwrap();
            let data = &v["data"];
            let fresh_refusal =
                fresh && v["effect_outcome"] == "not_attempted" && data["run"].is_null();
            let matched = data["run"]["request_id"] == o.fleet_request_id;
            if matched {
                o.fleet_run_id = data["run"]["id"].as_str().map(Into::into);
                o.fleet_aggregate = data["aggregate"].as_str().map(Into::into);
                o.fleet_effect_outcome = data["aggregate_effect_outcome"].as_str().map(Into::into);
            }
            o.state = if fresh_refusal {
                "failed"
            } else if matched && data["run"]["state"] == "terminal" {
                "terminal"
            } else {
                "unknown"
            }
            .into();
            o.reason = if o.state == "terminal" {
                None
            } else if fresh_refusal {
                Some(
                    v["errors"][0]["code"]
                        .as_str()
                        .unwrap_or("fleet_admission_refused")
                        .into(),
                )
            } else {
                Some("fleet_unresolved".into())
            };
            o.receipt_sha256 = Some(hash(&v));
            if serde_json::to_vec(&v).unwrap().len() <= 16384 {
                o.receipt = Some(v);
                o.receipt_omitted = false;
            } else {
                o.receipt = None;
                o.receipt_omitted = true;
            }
        }
    }
    o.observation_sha256 = Some(o.observation_fingerprint());
}
fn respond(
    r: &Request,
    state: &State,
    id: Option<&str>,
    action: &str,
    result: &mut OperationResult,
) -> Result<(), Fault> {
    let schedules: Vec<_> = state
        .schedules
        .iter()
        .filter(|s| id.is_none_or(|id| s.id == id))
        .collect();
    let mut occurrences: Vec<Value> = if matches!(action, "list" | "preview") {
        vec![]
    } else {
        schedules
            .iter()
            .flat_map(|s| s.occurrences.iter())
            .map(|o| {
                let mut v = serde_json::to_value(o).unwrap();
                v.as_object_mut().unwrap().remove("request_sha256");
                v.as_object_mut().unwrap().remove("last_work_sequence");
                v.as_object_mut().unwrap().remove("observation_sha256");
                v
            })
            .collect()
    };
    let budget = result.effective_limits.max_output_bytes;
    let mut used = 0usize;
    let mut details = occurrences.iter().any(|o| o["receipt_omitted"] == true);
    for o in &mut occurrences {
        if !o["receipt"].is_null() {
            let size = serde_json::to_vec(&o["receipt"]).unwrap().len();
            if used + size > budget {
                o["receipt"] = Value::Null;
                o["receipt_omitted"] = json!(true);
                details = true;
            } else {
                used += size;
            }
        }
    }
    let mut omitted = 0;
    while serde_json::to_vec(&occurrences).unwrap().len() > 512 * 1024 {
        if let Some(i) = occurrences.iter().position(|o| {
            o["state"] == "terminal" || o["state"] == "skipped" || o["state"] == "failed"
        }) {
            occurrences.remove(i);
            omitted += 1;
        } else {
            break;
        }
    }
    result.data = json!({"format_version":1,"action":action,"schedules":schedules.iter().map(|s|s.summary()).collect::<Vec<_>>(),"occurrences":occurrences,"preview":[],"omitted_occurrences":omitted,"cancellation_signal":null});
    result.effect_outcome = if result.effect_outcome == "succeeded" {
        "succeeded"
    } else {
        "not_applicable"
    }
    .into();
    result.execution.status = if details || omitted > 0 {
        Status::Partial
    } else {
        Status::Succeeded
    };
    result.coverage.state = if details || omitted > 0 {
        "partial"
    } else {
        "complete"
    }
    .into();
    if r.operation == "schedule.work"
        && schedules
            .iter()
            .any(|s| s.occurrences.iter().any(Occurrence::unresolved))
    {
        result.execution.status = Status::Unknown;
        result.coverage.state = "partial".into();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Schedule {
        let definition=config::definition(&json!({"enabled":true,"timezone":"UTC","cadence":{"kind":"interval","anchor":"2026-10-12T00:00:00Z","every_seconds":60},"collection":{"targets":["a"],"task":{"id":"error-budget","version":1,"input":{"slo":99.9,"bad_minutes":1}},"concurrency":1,"failure_threshold":1,"timeout_ms":30000},"misfire_grace_ms":1000,"retention_limit":128})).unwrap();
        let mut g = Generation {
            number: 1,
            definition,
            tzdb_version: model::TZDB.into(),
            binding: json!({"store":{},"selection":{}}),
            binding_sha256: String::new(),
            fleet_store: "/tmp/fleet".into(),
            inventory: "/tmp/inventory".into(),
        };
        g.binding_sha256 = g.fingerprint();
        let at = "2026-10-12T00:00:00Z".to_owned();
        Schedule {
            instance_id: crate::result::new_id("schedule"),
            id: "test".into(),
            owner_uid: model::uid(),
            revision: 1,
            generation: 1,
            enabled: true,
            created_at: at.clone(),
            updated_at: at,
            generations: vec![g],
            cursor: -1,
            last_skip: None,
            occurrences: vec![],
        }
    }
    fn at(s: &str) -> time::OffsetDateTime {
        time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).unwrap()
    }
    fn state(s: Schedule) -> State {
        State {
            work_sequence: 0,
            format_version: 1,
            owner_uid: model::uid(),
            schedules: vec![s],
        }
    }
    #[test]
    fn injected_clock_compresses_forward_jump_and_never_rewinds() {
        let mut s = fixture();
        assert!(plan(&mut s, at("2026-10-12T00:10:00Z")).unwrap());
        assert_eq!(s.cursor, 10);
        assert_eq!(s.last_skip.as_ref().unwrap().count, 10);
        assert_eq!(s.occurrences.len(), 1);
        assert_eq!(s.occurrences[0].ordinal, Some(10));
        assert!(!plan(&mut s, at("2026-10-12T00:05:00Z")).unwrap());
        assert_eq!(s.occurrences.len(), 1);
        assert!(plan(&mut s, at("2026-10-12T00:20:02Z")).unwrap());
        assert_eq!(s.cursor, 20);
        assert_eq!(s.last_skip.as_ref().unwrap().count, 10);
        assert_eq!(s.occurrences.len(), 1);
        assert!(s.valid());
    }
    #[test]
    fn pause_wins_before_atomic_intent_and_overlap_spans_generations() {
        let mut s = fixture();
        plan(&mut s, at("2026-10-12T00:00:00Z")).unwrap();
        s.enabled = false;
        assert!(!admit(&mut s, 0, 10000, at("2026-10-12T00:00:00Z")).unwrap());
        assert_eq!(s.occurrences[0].state, "queued");
        s.enabled = true;
        assert!(admit(&mut s, 0, 10000, at("2026-10-12T00:00:00Z")).unwrap());
        assert_eq!(s.occurrences[0].effective_timeout_ms, Some(6500));
        let mut g = s.current().clone();
        g.number = 2;
        g.binding_sha256 = g.fingerprint();
        s.generations.push(g);
        s.generation = 2;
        s.revision = 2;
        let mut manual = occurrence(&s, "manual", None, now(), None);
        manual.manual_request_id = Some("manual".into());
        manual.request_sha256 = Some(hash(&json!({"manual":1})));
        s.occurrences.push(manual);
        assert!(!admit(&mut s, 1, 30000, at("2026-10-12T00:00:00Z")).unwrap());
        assert_eq!(s.occurrences[0].effective_timeout_ms, Some(6500));
        assert!(s.valid());
    }
    #[test]
    fn nonexistent_daily_occurrence_never_gains_execution_intent() {
        let mut s = fixture();
        s.generations[0].definition["timezone"] = json!("America/New_York");
        s.generations[0].definition["cadence"] = json!({"kind":"daily","at":"02:30"});
        s.generations[0].binding_sha256 = s.generations[0].fingerprint();
        s.cursor = calendar::starting_cursor(
            &s.current().definition["cadence"],
            "America/New_York",
            at("2026-03-08T06:00:00Z"),
        )
        .unwrap();
        assert!(plan(&mut s, at("2026-03-08T07:00:00Z")).unwrap());
        assert_eq!(s.occurrences[0].state, "skipped");
        assert_eq!(
            s.occurrences[0].reason.as_deref(),
            Some("nonexistent_local_time")
        );
        assert!(!admit(&mut s, 0, 30000, at("2026-10-12T00:00:00Z")).unwrap());
    }
    #[test]
    fn private_store_lease_integrity_and_partial_publication() {
        let path = std::env::temp_dir().join(crate::result::new_id("schedule-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(10);
        let store = Store::open(&path, true, &c, d).unwrap();
        let lease = store.claim().unwrap();
        assert_eq!(store.claim().unwrap_err().code, "schedule_worker_busy");
        let mut state = state(fixture());
        store.commit(&state, &c, d).unwrap();
        assert!(store.load().unwrap().valid());
        state.schedules[0].enabled = false;
        store::FAIL_BEFORE_PUBLISH.with(|f| f.set(true));
        let err = store.commit(&state, &c, d).unwrap_err();
        assert!(err.effects);
        assert!(store.load().unwrap().schedules[0].enabled);
        drop(store);
        drop(lease);
        assert_eq!(
            Store::open(&path, false, &c, d).err().unwrap().code,
            "schedule_store_recovery_required"
        );
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn retained_receipt_mutation_and_non_object_state_fail_closed() {
        let mut s = fixture();
        plan(&mut s, at("2026-10-12T00:00:00Z")).unwrap();
        admit(&mut s, 0, 10000, at("2026-10-12T00:00:00Z")).unwrap();
        let o = &mut s.occurrences[0];
        o.state = "unknown".into();
        o.receipt = Some(json!({"data":1}));
        o.receipt_sha256 = Some(hash(o.receipt.as_ref().unwrap()));
        o.observation_sha256 = Some(o.observation_fingerprint());
        assert!(s.valid());
        s.occurrences[0].receipt = Some(json!({"data":2}));
        assert!(!s.valid());
    }
    #[test]
    fn manual_duplicate_returns_original_after_pause_without_cas_or_rebind() {
        let path = std::env::temp_dir().join(crate::result::new_id("schedule-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(10);
        let store = Store::open(&path, true, &c, d).unwrap();
        store.commit(&state(fixture()), &c, d).unwrap();
        drop(store);
        let context = OperatorContext {
            schedule_state_dir: Some(path.clone()),
            ..OperatorContext::default()
        };
        let mut request = Request::new(
            "schedule.run-now",
            json!({"id":"test","expected_revision":1,"request_id":"manual-1"}),
        );
        request.record = "always".into();
        let mut result = OperationResult::new(&request);
        run(&request, &c, &context, Instant::now(), &mut result, false).unwrap();
        assert_eq!(result.data["action"], "queued");
        let original = result.data["occurrences"][0]["id"].clone();
        let mut pause = Request::new("schedule.pause", json!({"id":"test","expected_revision":2}));
        pause.record = "always".into();
        run(
            &pause,
            &c,
            &context,
            Instant::now(),
            &mut OperationResult::new(&pause),
            false,
        )
        .unwrap();
        let mut result = OperationResult::new(&request);
        run(&request, &c, &context, Instant::now(), &mut result, false).unwrap();
        assert_eq!(result.data["action"], "existing");
        assert_eq!(result.data["occurrences"][0]["id"], original);
        assert_eq!(result.data["schedules"][0]["revision"], 3);
        request.inputs["expected_revision"] = json!(3);
        assert_eq!(
            run(
                &request,
                &c,
                &context,
                Instant::now(),
                &mut OperationResult::new(&request),
                false
            )
            .unwrap_err()
            .code,
            "schedule_request_conflict"
        );
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn immutable_terminal_and_hard_ledger_limit_do_not_erase_history() {
        let mut s = fixture();
        s.generations[0].definition["retention_limit"] = json!(1);
        s.generations[0].binding_sha256 = s.generations[0].fingerprint();
        plan(&mut s, at("2026-10-12T00:00:00Z")).unwrap();
        s.occurrences[0].state = "skipped".into();
        s.occurrences[0].reason = Some("overlap".into());
        assert_eq!(
            plan(&mut s, at("2026-10-12T00:01:00Z")).unwrap_err().code,
            "schedule_quota_exceeded"
        );
        assert_eq!(s.cursor, 0);
        assert_eq!(s.occurrences.len(), 1);
        assert!(s.valid());
    }
    #[test]
    fn old_automatic_row_expires_at_admission_and_manual_is_allowed_while_paused() {
        let mut s = fixture();
        plan(&mut s, at("2026-10-12T00:00:00Z")).unwrap();
        assert!(!admit(&mut s, 0, 30000, at("2026-10-13T00:00:00Z")).unwrap());
        assert_eq!(s.occurrences[0].state, "skipped");
        assert_eq!(s.occurrences[0].reason.as_deref(), Some("misfire"));
        let mut manual = occurrence(&s, "manual", None, now(), None);
        manual.manual_request_id = Some("paused-manual".into());
        manual.request_sha256 = Some(hash(&json!(1)));
        s.occurrences.push(manual);
        s.enabled = false;
        assert!(admit(&mut s, 1, 30000, at("2026-10-13T00:00:00Z")).unwrap());
        assert!(s.valid());
    }
    #[test]
    fn fresh_proven_refusal_resolves_but_recovery_not_found_remains_unknown() {
        let mut s = fixture();
        plan(&mut s, at("2026-10-12T00:00:00Z")).unwrap();
        admit(&mut s, 0, 30000, at("2026-10-12T00:00:00Z")).unwrap();
        let mut refusal = OperationResult::new(&Request::new("fleet.collect", json!({})));
        refusal.data = json!({"run":null});
        refusal.execution.status = Status::Failed;
        refusal.error("fleet_binding_changed", "Pinned profile changed.");
        observe(&mut s.occurrences[0], Ok(refusal), true);
        assert_eq!(s.occurrences[0].state, "failed");
        assert!(!s.occurrences[0].unresolved());
        assert!(s.valid());
        let mut next = occurrence(&s, "manual", None, now(), None);
        next.manual_request_id = Some("new".into());
        next.request_sha256 = Some(hash(&json!(2)));
        s.occurrences.push(next);
        assert!(admit(&mut s, 1, 30000, at("2026-10-12T00:00:00Z")).unwrap());
        let mut missing = OperationResult::new(&Request::new("fleet.resume", json!({})));
        missing.data = json!({"run":null});
        missing.error("fleet_not_found", "No matching parent.");
        observe(&mut s.occurrences[1], Ok(missing), false);
        assert_eq!(s.occurrences[1].state, "unknown");
        assert!(s.valid());
    }
    #[test]
    fn altered_aggregate_or_false_terminal_cannot_clear_persisted_overlap() {
        let mut s = fixture();
        plan(&mut s, at("2026-10-12T00:00:00Z")).unwrap();
        admit(&mut s, 0, 30000, at("2026-10-12T00:00:00Z")).unwrap();
        let mut receipt = OperationResult::new(&Request::new("fleet.resume", json!({})));
        receipt.data = json!({"run":{"id":"fleet-1","request_id":s.occurrences[0].fleet_request_id,"state":"recovery_required"},"aggregate":"unknown","aggregate_effect_outcome":"unknown"});
        observe(&mut s.occurrences[0], Ok(receipt), false);
        assert!(s.valid());
        let original = s.clone();
        s.occurrences[0].state = "terminal".into();
        assert!(!s.valid());
        s = original.clone();
        s.occurrences[0].fleet_aggregate = Some("succeeded".into());
        assert!(!s.valid());
        // Even an updated minimal summary hash cannot contradict a retained receipt.
        s.occurrences[0].observation_sha256 = Some(s.occurrences[0].observation_fingerprint());
        assert!(!s.valid());
    }
    #[test]
    fn distinct_schedule_instances_never_share_an_automatic_fleet_request() {
        let mut a = fixture();
        let mut b = fixture();
        plan(&mut a, at("2026-10-12T00:00:00Z")).unwrap();
        plan(&mut b, at("2026-10-12T00:00:00Z")).unwrap();
        assert_ne!(
            a.occurrences[0].fleet_request_id,
            b.occurrences[0].fleet_request_id
        );
        assert!(a.valid());
        assert!(b.valid());
    }
    #[test]
    fn unresolved_public_worker_receipt_has_required_outer_error() {
        let path = std::env::temp_dir().join(crate::result::new_id("schedule-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(10);
        let store = Store::open(&path, true, &c, d).unwrap();
        let mut s = fixture();
        s.enabled = false;
        let mut o = occurrence(&s, "manual", None, now(), None);
        o.manual_request_id = Some("queued".into());
        o.request_sha256 = Some(hash(&json!(1)));
        s.occurrences.push(o);
        store.commit(&state(s), &c, d).unwrap();
        drop(store);
        let context = OperatorContext {
            schedule_state_dir: Some(path.clone()),
            ..OperatorContext::default()
        };
        let mut request = Request::new("schedule.work", json!({"max_occurrences":1}));
        request.record = "always".into();
        let receipt = crate::schedule::work(request, &c, &context);
        assert_eq!(receipt.execution.status, Status::Unknown);
        assert!(!receipt.errors.is_empty());
        assert_eq!(receipt.data["occurrences"][0]["state"], "unknown");
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn metadata_and_receipt_reservation_survives_near_cap_terminal_publication() {
        let mut s = fixture();
        let mut o = occurrence(&s, "manual", None, now(), None);
        o.manual_request_id = Some("reserved".into());
        o.request_sha256 = Some(hash(&json!(1)));
        s.occurrences.push(o);
        s.generations[0].binding["selection"]["padding"] = json!("");
        let mut state = state(s);
        let max = 31 * 1024 * 1024;
        let headroom = max - store::reserved_size(&state).unwrap();
        state.schedules[0].generations[0].binding["selection"]["padding"] =
            json!("a".repeat(headroom));
        state.schedules[0].generations[0].binding_sha256 =
            state.schedules[0].generations[0].fingerprint();
        assert_eq!(store::reserved_size(&state), Some(max));
        let o = &mut state.schedules[0].occurrences[0];
        o.state = "intent".into();
        o.effective_timeout_ms = Some(30000);
        let mut receipt = OperationResult::new(&Request::new("fleet.collect", json!({})));
        receipt.data = json!({"run":{"id":"fleet-complete","request_id":o.fleet_request_id,"state":"terminal"},"aggregate":"succeeded","aggregate_effect_outcome":"not_applicable","detail":"z".repeat(12000)});
        observe(o, Ok(receipt), true);
        assert_eq!(o.state, "terminal");
        assert!(o.receipt.is_some());
        assert_eq!(store::reserved_size(&state), Some(max));
        assert!(state.valid());
    }
    #[test]
    fn persistent_work_rotation_does_not_starve_a_later_schedule() {
        let path = std::env::temp_dir().join(crate::result::new_id("schedule-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(10);
        let store = Store::open(&path, true, &c, d).unwrap();
        let mut first = fixture();
        first.enabled = false;
        let mut old = occurrence(&first, "manual", None, now(), None);
        old.manual_request_id = Some("old".into());
        old.request_sha256 = Some(hash(&json!(1)));
        old.state = "intent".into();
        old.effective_timeout_ms = Some(1000);
        old.last_work_sequence = 1;
        observe(&mut old, Err("interrupted"), false);
        first.occurrences.push(old);
        let mut second = fixture();
        second.id = "later".into();
        second.enabled = false;
        let mut queued = occurrence(&second, "manual", None, now(), None);
        queued.manual_request_id = Some("next".into());
        queued.request_sha256 = Some(hash(&json!(2)));
        second.occurrences.push(queued);
        let state = State {
            format_version: 1,
            owner_uid: model::uid(),
            work_sequence: 1,
            schedules: vec![first, second],
        };
        store.commit(&state, &c, d).unwrap();
        drop(store);
        let context = OperatorContext {
            schedule_state_dir: Some(path.clone()),
            ..OperatorContext::default()
        };
        let mut request = Request::new("schedule.work", json!({"max_occurrences":1}));
        request.record = "always".into();
        let _receipt = crate::schedule::work(request, &c, &context);
        let store = Store::open(&path, false, &c, d).unwrap();
        let state = store.load().unwrap();
        assert_eq!(state.schedules[0].occurrences[0].last_work_sequence, 1);
        assert_eq!(state.schedules[1].occurrences[0].last_work_sequence, 2);
        drop(store);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn live_cancel_poll_reads_current_intent_not_the_dispatch_clone() {
        let path = std::env::temp_dir().join(crate::result::new_id("schedule-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(10);
        let store = Store::open(&path, true, &c, d).unwrap();
        let mut s = fixture();
        plan(&mut s, at("2026-10-12T00:00:00Z")).unwrap();
        admit(&mut s, 0, 30000, at("2026-10-12T00:00:00Z")).unwrap();
        let stale = s.occurrences[0].clone();
        let mut state = state(s);
        store.commit(&state, &c, d).unwrap();
        state.schedules[0].occurrences[0].cancel_requested = true;
        store.commit(&state, &c, d).unwrap();
        drop(store);
        let context = OperatorContext {
            schedule_state_dir: Some(path.clone()),
            ..OperatorContext::default()
        };
        assert!(!stale.cancel_requested);
        assert!(cancellation_requested(&context, &stale, &c, d));
        std::fs::remove_dir_all(path).unwrap();
    }
}
