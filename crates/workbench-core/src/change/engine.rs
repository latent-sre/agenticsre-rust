use super::{
    decode,
    fs::{Observation, Target},
    hash,
    ledger::{Approval, Ledger, RECOVERY, Record},
    pin, valid_input,
};
use crate::{
    OperatorContext, RunControl,
    request::Request,
    result::{OperationResult, Status},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Component, Path, PathBuf},
    sync::{Mutex, atomic::Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    state_dir: String,
    targets: Vec<Grant>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    id: String,
    path: String,
    validator_binding_sha256: String,
    requesters: Vec<String>,
    approvers: Vec<String>,
}
fn path(v: &str) -> bool {
    Path::new(v).is_absolute()
        && v.len() <= 1024
        && !v.chars().any(char::is_control)
        && Path::new(v)
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
}
pub(crate) fn valid_config(value: &Value, principals: &[String]) -> bool {
    let Ok(cfg) = serde_json::from_value::<Config>(value.clone()) else {
        return false;
    };
    if !path(&cfg.state_dir) || cfg.targets.is_empty() || cfg.targets.len() > 16 {
        return false;
    }
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    cfg.targets.iter().all(|g| {
        let requesters = g.requesters.iter().collect::<BTreeSet<_>>();
        let approvers = g.approvers.iter().collect::<BTreeSet<_>>();
        crate::request::valid_operation(&g.id)
            && ids.insert(&g.id)
            && path(&g.path)
            && paths.insert(&g.path)
            && pin(&g.validator_binding_sha256)
            && !requesters.is_empty()
            && !approvers.is_empty()
            && requesters.len() == g.requesters.len()
            && approvers.len() == g.approvers.len()
            && requesters.is_disjoint(&approvers)
            && g.requesters
                .iter()
                .chain(&g.approvers)
                .all(|p| principals.contains(p))
    })
}
fn implementation() -> String {
    hash(
        &[
            include_bytes!("../change.rs").as_slice(),
            include_bytes!("engine.rs").as_slice(),
            include_bytes!("ledger.rs").as_slice(),
            include_bytes!("fs.rs").as_slice(),
        ]
        .concat(),
    )
}
fn wall() -> Result<u64, &'static str> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| "change_clock_invalid")
}
fn active(control: &RunControl, deadline: Instant) -> Result<(), &'static str> {
    if control.signal.load(Ordering::Relaxed) != 0 {
        Err("change_cancelled")
    } else if Instant::now() >= deadline {
        Err("change_deadline_exceeded")
    } else {
        Ok(())
    }
}
pub(crate) struct Service {
    config: Config,
    runner: Value,
    principals: BTreeMap<String, String>,
    promtool: Option<PathBuf>,
    ledger: Mutex<Ledger>,
}
impl Service {
    pub(crate) fn new(
        config: &Value,
        runner_id: &str,
        cert: &str,
        policy: &str,
        promtool: Option<&Path>,
    ) -> Result<Self, &'static str> {
        let cfg: Config = serde_json::from_value(config["changes"].clone())
            .map_err(|_| "invalid_change_config")?;
        let principals: BTreeMap<String, String> = config["principals"]
            .as_array()
            .ok_or("invalid_change_config")?
            .iter()
            .map(|p| {
                Ok((
                    p["id"].as_str().ok_or("invalid_change_config")?.to_owned(),
                    p["certificate_sha256"]
                        .as_str()
                        .ok_or("invalid_change_config")?
                        .to_owned(),
                ))
            })
            .collect::<Result<_, &'static str>>()?;
        if !valid_config(
            &config["changes"],
            &principals.keys().cloned().collect::<Vec<_>>(),
        ) {
            return Err("invalid_change_config");
        }
        // Refuse lexical aliases and hardlinks to one file before accepting requests.
        let mut identities = BTreeSet::new();
        for grant in &cfg.targets {
            let target = Target::open(Path::new(&grant.path))?;
            let (snapshot, _) = target.snapshot()?;
            if !identities.insert((snapshot.file.dev, snapshot.file.ino)) {
                return Err("invalid_change_config");
            }
        }
        let ledger = Ledger::open(Path::new(&cfg.state_dir), runner_id)?;
        Ok(Self {
            config: cfg,
            runner: json!({"id":runner_id,"server_certificate_sha256":cert,"policy_sha256":policy}),
            principals,
            promtool: promtool.map(Path::to_path_buf),
            ledger: Mutex::new(ledger),
        })
    }
    fn grant(&self, id: &str) -> Result<&Grant, &'static str> {
        self.config
            .targets
            .iter()
            .find(|g| g.id == id)
            .ok_or("change_grant_denied")
    }
    fn authorized(&self, principal: &str, requester: &str, grant: &Grant, method: &str) -> bool {
        grant.requesters.iter().any(|p| p == requester)
            && if method == "approve" {
                principal != requester && grant.approvers.iter().any(|p| p == principal)
            } else if method == "plan" || method == "apply" {
                principal == requester
            } else {
                principal == requester || grant.approvers.iter().any(|p| p == principal)
            }
    }
    fn fresh(&self, ledger: &Ledger, key: &str, record: &Record, now: u64) -> bool {
        let Some(anchor) = ledger.anchors.get(key) else {
            return false;
        };
        now >= ledger.last_wall
            && now >= record.plan["created_at"].as_u64().unwrap_or(u64::MAX)
            && now < record.plan["expires_at"].as_u64().unwrap_or(0)
            && anchor.elapsed()
                < Duration::from_secs(record.plan["expires_in_seconds"].as_u64().unwrap_or(0))
    }
    fn validate(
        &self,
        ledger: &Ledger,
        bytes: &[u8],
        grant: &Grant,
        control: &RunControl,
        deadline: Instant,
    ) -> Result<Value, &'static str> {
        active(control, deadline)?;
        let name = crate::result::new_id("validation");
        let mut file = ledger.directory.create_file(&name).map_err(|_| RECOVERY)?;
        let result = (|| {
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| RECOVERY)?;
            let input = json!({"kind":"prometheus-rules","validator":{"id":"promtool","version":1,"expected_binding_sha256":grant.validator_binding_sha256},"file":ledger.path.join(&name),"expected_input_sha256":hash(bytes)});
            let mut request = Request::new("config.check", input);
            request.limits.timeout_ms = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(30000) as u64;
            request.limits.max_output_bytes = 32768;
            let mut receipt = OperationResult::new(&request);
            let validation_start = Instant::now();
            let context = OperatorContext {
                promtool_path: self.promtool.clone(),
                ..OperatorContext::default()
            };
            crate::tasks::config_check::run(
                &request,
                control,
                &context,
                validation_start,
                &mut receipt,
            );
            receipt.finish(validation_start);
            if receipt.execution.status != Status::Succeeded
                || receipt.coverage.state != "complete"
                || receipt.data["validation_state"] != "complete"
                || receipt.data["validation_outcome"] != "valid"
                || receipt.data["findings_total"] != 0
                || !receipt.data["findings"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
                || !receipt.data["checked_rules"]
                    .as_u64()
                    .is_some_and(|n| n > 0)
                || receipt.data["input"]["digest"] != hash(bytes)
                || receipt.data["validator"]["binding_digest"] != grant.validator_binding_sha256
            {
                return Err("change_validation_failed");
            }
            serde_json::to_value(receipt).map_err(|_| RECOVERY)
        })();
        ledger
            .directory
            .unlink(&name, false)
            .and_then(|_| ledger.directory.sync())
            .map_err(|_| RECOVERY)?;
        result
    }
    pub(crate) fn handle(
        &self,
        principal: &str,
        certificate: &str,
        method: &str,
        input: &Value,
        control: &RunControl,
        deadline: Instant,
    ) -> Value {
        let method = method.strip_prefix("change.").unwrap_or(method);
        let requester = if method == "plan" || method == "apply" {
            principal
        } else {
            input["requester"].as_str().unwrap_or("")
        };
        let mut data = json!({"format_version":1,"action":method,"runner":self.runner,"principal":{"id":principal,"client_certificate_sha256":certificate},"requester":null,"request_id":null,"target":null,"plan":null,"plan_sha256":null,"approval_id":null,"state":"not_found","effect_outcome":"not_attempted","remote_error":null});
        let outcome = (|| -> Result<(), &'static str> {
            if !valid_input(method, input) {
                return Err("change_invalid_input");
            }
            data["request_id"] = input["request_id"].clone();
            if self.principals.get(principal).map(String::as_str) != Some(certificate) {
                return Err("change_grant_denied");
            }
            active(control, deadline)?;
            let mut ledger = self.ledger.try_lock().map_err(|_| "change_store_busy")?;
            ledger.check()?;
            let now = wall()?;
            if now < ledger.last_wall {
                ledger.anchors.clear();
                return Err("change_clock_invalid");
            }
            ledger.last_wall = now;
            let request_id = input["request_id"].as_str().unwrap();
            let key = hash(&serde_json::to_vec(&(requester, request_id)).unwrap());
            let existing = ledger.records().get(&key).cloned();
            let grant = if let Some(record) = &existing {
                self.grant(record.plan["target"]["id"].as_str().ok_or(RECOVERY)?)?
            } else if method == "plan" {
                self.grant(input["target"].as_str().unwrap())?
            } else {
                // Authorization is checked before revealing whether this requester exists.
                if !self
                    .config
                    .targets
                    .iter()
                    .any(|g| self.authorized(principal, requester, g, method))
                {
                    return Err("change_grant_denied");
                }
                data["requester"] = json!(requester);
                data["request_id"] = json!(request_id);
                return Ok(());
            };
            if !self.authorized(principal, requester, grant, method) {
                if matches!(method, "status" | "approve" | "reconcile")
                    && self
                        .config
                        .targets
                        .iter()
                        .any(|g| self.authorized(principal, requester, g, method))
                {
                    // An observer authorized on another target sees the same absent result.
                    data["requester"] = json!(requester);
                    return Ok(());
                }
                return Err("change_grant_denied");
            }
            data["requester"] = json!(requester);
            data["request_id"] = json!(request_id);
            data["target"] = json!(grant.id);
            let mut record = if let Some(record) = existing {
                render(&mut data, &record, self.fresh(&ledger, &key, &record, now));
                record
            } else {
                if method != "plan" {
                    return Ok(());
                }
                if ledger.records().len() >= 32 {
                    return Err("change_store_full");
                }
                let target = Target::open(Path::new(&grant.path))?;
                let (original, bytes) = target.snapshot()?;
                let proposed = decode(input["proposed_base64"].as_str().unwrap()).unwrap();
                if hash(&bytes) == hash(&proposed) {
                    return Err("change_noop");
                }
                let validation = self.validate(&ledger, &proposed, grant, control, deadline)?;
                active(control, deadline)?;
                if !target.snapshot()?.0.matches(&original) {
                    return Err("change_precondition_failed");
                }
                let ttl = input["expires_in_seconds"].as_u64().unwrap();
                let plan = json!({"format_version":1,"kind":"prometheus-rules-file-replacement","requester":requester,"requester_certificate_sha256":certificate,"request_id":request_id,"target":{"id":grant.id,"path":grant.path},"runner":self.runner,"ledger":ledger.owner(),"original_identity":original,"original_base64":STANDARD.encode(&bytes),"original_sha256":hash(&bytes),"proposed_base64":input["proposed_base64"],"proposed_sha256":hash(&proposed),"validator_binding_sha256":grant.validator_binding_sha256,"implementation_sha256":implementation(),"validation":validation,"created_at":now,"expires_at":now+ttl,"expires_in_seconds":ttl,"readback":"prepared_inode_and_exact_bytes","recovery":"new_approved_plan_using_original_bytes"});
                let record = Record {
                    plan_sha256: hash(&serde_json::to_vec(&plan).unwrap()),
                    plan,
                    approval: None,
                    consumed: false,
                    prepared: None,
                    state: "planned".into(),
                    effect_outcome: "not_attempted".into(),
                };
                ledger.insert(key.clone(), record.clone());
                ledger.anchors.insert(key.clone(), Instant::now());
                ledger.save()?;
                record
            };
            if method == "plan"
                && (record.plan["target"]["id"] != input["target"]
                    || record.plan["proposed_base64"] != input["proposed_base64"]
                    || record.plan["expires_in_seconds"] != input["expires_in_seconds"]
                    || record.plan["requester_certificate_sha256"] != certificate)
            {
                return Err("change_request_conflict");
            }
            // Public observations of a consumed attempt are retained even after expiry/restart.
            let fresh = self.fresh(&ledger, &key, &record, now);
            if method == "approve" || method == "apply" {
                if record.plan_sha256 != input["plan_sha256"].as_str().unwrap() {
                    return Err("change_plan_mismatch");
                }
                if !record.consumed && !fresh {
                    return Err("change_expired");
                }
                if record.plan["runner"] != self.runner
                    || record.plan["ledger"] != ledger.owner()
                    || record.plan["target"]["path"] != grant.path
                    || record.plan["validator_binding_sha256"] != grant.validator_binding_sha256
                    || record.plan["implementation_sha256"] != implementation()
                    || self.principals.get(requester).map(String::as_str)
                        != record.plan["requester_certificate_sha256"].as_str()
                {
                    return Err("change_precondition_failed");
                }
            }
            match method {
                "approve" => {
                    if record.consumed {
                        return Err("change_approval_consumed");
                    }
                    if let Some(approval) = &record.approval {
                        if approval.principal != principal
                            || approval.certificate_sha256 != certificate
                        {
                            return Err("change_already_approved");
                        }
                    } else {
                        record.approval = Some(Approval {
                            id: hash(crate::result::new_id("approval").as_bytes()),
                            principal: principal.to_owned(),
                            certificate_sha256: certificate.to_owned(),
                        });
                        record.state = "approved".into();
                        ledger.insert(key.clone(), record.clone());
                        ledger.save()?;
                    }
                }
                "apply" => {
                    let approval = record.approval.as_ref().ok_or("change_approval_required")?;
                    if approval.id != input["approval_id"].as_str().unwrap()
                        || approval.principal == principal
                        || !grant.approvers.contains(&approval.principal)
                        || self.principals.get(&approval.principal)
                            != Some(&approval.certificate_sha256)
                        || record.plan["requester_certificate_sha256"] != certificate
                    {
                        return Err("change_approval_mismatch");
                    }
                    if !record.consumed {
                        let target = Target::open(Path::new(&grant.path))?;
                        let original = record.original()?;
                        if !target.snapshot()?.0.matches(&original) {
                            return Err("change_precondition_failed");
                        }
                        let proposed =
                            decode(record.plan["proposed_base64"].as_str().ok_or(RECOVERY)?)
                                .ok_or(RECOVERY)?;
                        self.validate(&ledger, &proposed, grant, control, deadline)?;
                        active(control, deadline)?;
                        if !self.fresh(&ledger, &key, &record, wall()?) {
                            return Err("change_expired");
                        }
                        let prepared = target.prepare(
                            &proposed,
                            &original,
                            &crate::result::new_id("change"),
                        )?;
                        active(control, deadline)?;
                        if !self.fresh(&ledger, &key, &record, wall()?) {
                            return Err("change_expired");
                        }
                        record.consumed = true;
                        record.prepared = Some(prepared.clone());
                        record.state = "dispatching".into();
                        record.effect_outcome = "unknown".into();
                        render(&mut data, &record, true);
                        ledger.insert(key.clone(), record.clone());
                        ledger.save()?;
                        // The durable consumed approval and dispatch intent precede the only rename.
                        active(control, deadline)?;
                        if !self.fresh(&ledger, &key, &record, wall()?) {
                            return Err("change_expired");
                        }
                        let observation = if target.replace(&original, &prepared).is_ok() {
                            target
                                .observe(&original, &prepared)
                                .unwrap_or(Observation::Unknown)
                        } else {
                            Observation::Unknown
                        };
                        match observation {
                            Observation::Replacement => {
                                record.state = "verified".into();
                                record.effect_outcome = "verified".into();
                            }
                            Observation::Original => {
                                record.state = "not_applied".into();
                                record.effect_outcome = "unknown".into();
                            }
                            Observation::Unknown => {
                                record.state = "unknown".into();
                                record.effect_outcome = "unknown".into();
                            }
                        }
                        ledger.insert(key.clone(), record.clone());
                        ledger.save()?;
                    }
                }
                "reconcile" if record.consumed => {
                    let prepared = record.prepared.as_ref().ok_or(RECOVERY)?;
                    let observation = match Target::open(Path::new(&grant.path)) {
                        Ok(target) if target.sync().is_ok() => target
                            .observe(&record.original()?, prepared)
                            .unwrap_or(Observation::Unknown),
                        _ => Observation::Unknown,
                    };
                    match observation {
                        Observation::Replacement => {
                            record.state = "verified".into();
                            record.effect_outcome = "verified".into();
                        }
                        Observation::Original => {
                            record.state = "not_applied".into();
                            record.effect_outcome = "unknown".into();
                        }
                        Observation::Unknown => {
                            record.state = "unknown".into();
                            record.effect_outcome = "unknown".into();
                        }
                    }
                    render(&mut data, &record, fresh);
                    ledger.insert(key.clone(), record.clone());
                    ledger.save()?;
                }
                _ => {}
            }
            render(&mut data, &record, fresh);
            Ok(())
        })();
        data["remote_error"] = outcome
            .as_ref()
            .err()
            .map(|e| json!(e))
            .unwrap_or(Value::Null);
        json!({"protocol":1,"ok":outcome.is_ok(),"error":outcome.err(),"data":data})
    }
}

fn render(data: &mut Value, record: &Record, fresh: bool) {
    data["plan"] = record.plan.clone();
    data["plan_sha256"] = json!(record.plan_sha256);
    data["approval_id"] = record
        .approval
        .as_ref()
        .map(|a| json!(a.id))
        .unwrap_or(Value::Null);
    data["state"] = json!(if !record.consumed && !fresh {
        "expired"
    } else {
        record.state.as_str()
    });
    data["effect_outcome"] = json!(record.effect_outcome);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Fixture {
        root: PathBuf,
        config: Value,
        service: Service,
    }
    impl Fixture {
        fn new() -> Self {
            Self::new_with_state(None)
        }
        fn new_with_state(state: Option<PathBuf>) -> Self {
            let root = std::env::temp_dir().join(crate::result::new_id("change-engine"));
            std::fs::create_dir(&root).unwrap();
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            let target = root.join("rules.yml");
            std::fs::write(
                &target,
                b"groups:\n- name: original\n  rules:\n  - record: original:up\n    expr: up\n",
            )
            .unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
            let request = Request::new(
                "config.check",
                json!({"kind":"prometheus-rules","validator":{"id":"promtool","version":1},"file":"/unused"}),
            );
            let mut result = OperationResult::new(&request);
            crate::tasks::config_check::run(
                &request,
                &RunControl::default(),
                &OperatorContext::default(),
                Instant::now(),
                &mut result,
            );
            let cert = hash(b"requester");
            let config = json!({"principals":[{"id":"requester","certificate_sha256":cert},{"id":"approver","certificate_sha256":hash(b"approver")},{"id":"outsider","certificate_sha256":hash(b"outsider")}],"changes":{"state_dir":state.unwrap_or_else(||root.join("ledger")),"targets":[{"id":"rules","path":target,"validator_binding_sha256":result.data["validator"]["binding_digest"],"requesters":["requester"],"approvers":["approver"]}]}});
            let promtool = std::env::var_os("WORKBENCH_TEST_PROMTOOL").map(PathBuf::from);
            let service = Service::new(
                &config,
                "runner",
                &hash(b"server"),
                &hash(b"policy"),
                promtool.as_deref(),
            )
            .unwrap();
            Self {
                root,
                config,
                service,
            }
        }
        fn call(&self, principal: &str, method: &str, input: Value) -> Value {
            self.service.handle(
                principal,
                &hash(principal.as_bytes()),
                method,
                &input,
                &RunControl::default(),
                Instant::now() + Duration::from_secs(30),
            )
        }
        fn plan(&self) -> Value {
            self.call("requester","plan",json!({"request_id":"example","target":"rules","proposed_base64":STANDARD.encode(b"groups:\n- name: proposed\n  rules:\n  - record: proposed:up\n    expr: up\n"),"expires_in_seconds":900}))
        }
        fn approve(&self, plan: &Value) -> Value {
            self.call("approver","approve",json!({"requester":"requester","request_id":"example","plan_sha256":plan["data"]["plan_sha256"]}))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    #[ignore = "requires exact reviewed promtool; run with WORKBENCH_TEST_PROMTOOL"]
    fn exact_approval_applies_once_and_reconciles_retained_inode() {
        let fixture = Fixture::new();
        let plan = fixture.plan();
        assert_eq!(plan["ok"], true, "{plan}");
        let denied = fixture.call(
            "outsider",
            "status",
            json!({"requester":"requester","request_id":"example"}),
        );
        assert_eq!(denied["error"], "change_grant_denied");
        assert!(denied["data"]["plan"].is_null());
        let own = fixture.call("requester","approve",json!({"requester":"requester","request_id":"example","plan_sha256":plan["data"]["plan_sha256"]}));
        assert_eq!(own["error"], "change_grant_denied");
        let approval = fixture.approve(&plan);
        assert_eq!(approval["ok"], true, "{approval}");
        let input = json!({"request_id":"example","plan_sha256":plan["data"]["plan_sha256"],"approval_id":approval["data"]["approval_id"]});
        let applied = fixture.call("requester", "apply", input.clone());
        assert_eq!(applied["data"]["state"], "verified", "{applied}");
        let first = Target::open(&fixture.root.join("rules.yml"))
            .unwrap()
            .snapshot()
            .unwrap()
            .0;
        let repeated = fixture.call("requester", "apply", input);
        assert_eq!(repeated["data"]["state"], "verified", "{repeated}");
        let second = Target::open(&fixture.root.join("rules.yml"))
            .unwrap()
            .snapshot()
            .unwrap()
            .0;
        assert_eq!(
            first.file, second.file,
            "Replay must not perform another rename"
        );
        let reconcile = fixture.call(
            "approver",
            "reconcile",
            json!({"requester":"requester","request_id":"example"}),
        );
        assert_eq!(reconcile["data"]["state"], "verified");
        std::fs::write(fixture.root.join("rules.yml"), b"unrelated content").unwrap();
        let unknown = fixture.call(
            "requester",
            "reconcile",
            json!({"requester":"requester","request_id":"example"}),
        );
        assert_eq!(unknown["data"]["state"], "unknown");
    }
    #[test]
    #[ignore = "requires exact reviewed promtool and tools/test-sandbox.sh --small-evidence-disk"]
    fn reconcile_save_failure_keeps_current_unknown_observation() {
        let _disk_guard = crate::change::ledger::DISK_TEST_LEASE.lock().unwrap();
        let mount = Path::new("/tmp/evidence-small-disk");
        assert!(
            mount.is_dir(),
            "Use the declared small private disk sandbox"
        );
        let state = mount.join(crate::result::new_id("reconcile-state"));
        let fixture = Fixture::new_with_state(Some(state.clone()));
        let plan = fixture.plan();
        assert_eq!(plan["ok"], true, "{plan}");
        let approval = fixture.approve(&plan);
        assert_eq!(approval["ok"], true, "{approval}");
        let applied = fixture.call("requester","apply",json!({"request_id":"example","plan_sha256":plan["data"]["plan_sha256"],"approval_id":approval["data"]["approval_id"]}));
        assert_eq!(applied["data"]["state"], "verified", "{applied}");
        std::fs::write(
            fixture.root.join("rules.yml"),
            b"unknown external modification",
        )
        .unwrap();
        let fill = mount.join(crate::result::new_id("reconcile-fill"));
        let mut filler = std::fs::File::create(&fill).unwrap();
        let mut exhausted = false;
        for _ in 0..64 {
            if filler.write_all(&[0; 4096]).is_err() {
                exhausted = true;
                break;
            }
        }
        assert!(exhausted);
        let response = fixture.call(
            "requester",
            "reconcile",
            json!({"requester":"requester","request_id":"example"}),
        );
        assert_eq!(response["error"], RECOVERY, "{response}");
        assert_eq!(response["data"]["state"], "unknown", "{response}");
        assert_eq!(response["data"]["effect_outcome"], "unknown", "{response}");
        assert_eq!(
            std::fs::read(fixture.root.join("rules.yml")).unwrap(),
            b"unknown external modification"
        );
        drop(filler);
        std::fs::remove_file(fill).unwrap();
        std::fs::remove_dir_all(state).unwrap();
    }
    #[test]
    #[ignore = "requires exact reviewed promtool; run with WORKBENCH_TEST_PROMTOOL"]
    fn expiration_restart_and_changed_preconditions_do_not_dispatch() {
        let mut fixture = Fixture::new();
        let plan = fixture.plan();
        assert_eq!(plan["ok"], true, "{plan}");
        let approval = fixture.approve(&plan);
        assert_eq!(approval["ok"], true, "{approval}");
        let input = json!({"request_id":"example","plan_sha256":plan["data"]["plan_sha256"],"approval_id":approval["data"]["approval_id"]});
        let original = std::fs::read(fixture.root.join("rules.yml")).unwrap();
        fixture.service.ledger.lock().unwrap().anchors.clear();
        assert_eq!(
            fixture.call("requester", "apply", input.clone())["error"],
            "change_expired"
        );
        assert_eq!(
            std::fs::read(fixture.root.join("rules.yml")).unwrap(),
            original
        );
        let key = hash(&serde_json::to_vec(&("requester", "example")).unwrap());
        fixture
            .service
            .ledger
            .lock()
            .unwrap()
            .anchors
            .insert(key, Instant::now());
        std::fs::write(fixture.root.join("rules.yml"), b"changed").unwrap();
        assert_eq!(
            fixture.call("requester", "apply", input)["error"],
            "change_precondition_failed"
        );
        fixture.service.ledger.lock().unwrap().last_wall = u64::MAX;
        assert_eq!(
            fixture.call(
                "requester",
                "status",
                json!({"requester":"requester","request_id":"example"})
            )["error"],
            "change_clock_invalid"
        );
        // Reopening with identical identity must preserve observations but lose write anchors.
        let mut placeholder = Fixture::new();
        std::mem::swap(&mut fixture.service, &mut placeholder.service);
        drop(placeholder);
        fixture.service = Service::new(
            &fixture.config,
            "runner",
            &hash(b"server"),
            &hash(b"policy"),
            None,
        )
        .unwrap();
        let status = fixture.call(
            "requester",
            "status",
            json!({"requester":"requester","request_id":"example"}),
        );
        assert_eq!(status["data"]["state"], "expired", "{status}");
    }
    #[test]
    fn mutex_contention_and_corrupt_ledger_fail_closed() {
        let fixture = Fixture::new();
        let locked = fixture.service.ledger.lock().unwrap();
        assert_eq!(
            fixture.call(
                "requester",
                "status",
                json!({"requester":"requester","request_id":"example"})
            )["error"],
            "change_store_busy"
        );
        drop(locked);
        std::fs::write(fixture.root.join("ledger/ledger.json"), b"{").unwrap();
        assert_eq!(fixture.plan()["error"], RECOVERY);
        assert_eq!(
            fixture.call(
                "requester",
                "status",
                json!({"requester":"requester","request_id":"example"})
            )["error"],
            RECOVERY
        );
        assert!(
            !std::fs::read(fixture.root.join("rules.yml"))
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn config_rejects_overlap_unknown_principals_aliases_and_bad_pins() {
        let fixture = Fixture::new();
        let principals = vec!["requester".to_owned(), "approver".to_owned()];
        for (field, value) in [
            ("approvers", json!(["requester"])),
            ("requesters", json!(["unknown"])),
            ("validator_binding_sha256", json!("bad")),
        ] {
            let mut config = fixture.config["changes"].clone();
            config["targets"][0][field] = value;
            assert!(!valid_config(&config, &principals));
        }
        let mut config = fixture.config["changes"].clone();
        let mut alias = config["targets"][0].clone();
        alias["id"] = json!("alias");
        config["targets"].as_array_mut().unwrap().push(alias);
        assert!(!valid_config(&config, &principals));
    }
    #[test]
    fn no_op_and_backwards_wall_clock_refuse_before_validator() {
        let fixture = Fixture::new();
        let original = std::fs::read(fixture.root.join("rules.yml")).unwrap();
        let input = json!({"request_id":"noop","target":"rules","proposed_base64":STANDARD.encode(&original),"expires_in_seconds":900});
        assert_eq!(
            fixture.call("requester", "plan", input)["error"],
            "change_noop"
        );
        assert!(fixture.service.ledger.lock().unwrap().records().is_empty());
        fixture.service.ledger.lock().unwrap().last_wall = u64::MAX;
        assert_eq!(fixture.plan()["error"], "change_clock_invalid");
        assert_eq!(
            std::fs::read(fixture.root.join("rules.yml")).unwrap(),
            original
        );
    }
    #[test]
    #[ignore = "requires exact reviewed promtool; run with WORKBENCH_TEST_PROMTOOL"]
    fn exact_digest_mutation_and_plan_quota_refuse_without_replacement() {
        let fixture = Fixture::new();
        let plan = fixture.plan();
        assert_eq!(plan["ok"], true, "{plan}");
        let wrong=fixture.call("approver","approve",json!({"requester":"requester","request_id":"example","plan_sha256":hash(b"different-plan")}));
        assert_eq!(wrong["error"], "change_plan_mismatch");
        let mut changed = json!({"request_id":"example","target":"rules","proposed_base64":STANDARD.encode(b"other bytes"),"expires_in_seconds":900});
        assert_eq!(
            fixture.call("requester", "plan", changed.clone())["error"],
            "change_request_conflict"
        );
        let key = hash(&serde_json::to_vec(&("requester", "example")).unwrap());
        {
            let mut ledger = fixture.service.ledger.lock().unwrap();
            let record = ledger.records()[&key].clone();
            for i in 1..32 {
                let mut extra = record.clone();
                extra.plan["request_id"] = json!(format!("record-{i}"));
                extra.plan_sha256 = hash(&serde_json::to_vec(&extra.plan).unwrap());
                let key = hash(&serde_json::to_vec(&("requester", format!("record-{i}"))).unwrap());
                ledger.insert(key, extra);
            }
            ledger.save().unwrap();
        }
        changed["request_id"] = json!("exceeds-quota");
        assert_eq!(
            fixture.call("requester", "plan", changed)["error"],
            "change_store_full"
        );
        assert_eq!(fixture.service.ledger.lock().unwrap().records().len(), 32);
    }
}
