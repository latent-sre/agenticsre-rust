use super::*;
use crate::evidence::store::fs::{self, Directory};
use config::Server;
use std::{
    collections::BTreeMap, io::Write, net::TcpListener, os::unix::fs::MetadataExt, path::PathBuf,
    sync::atomic::Ordering,
};
fn storage<T>(r: Result<T, crate::evidence::store::StoreError>) -> Result<T, RunnerError> {
    r.map_err(|_| "runner_store_recovery_required".into())
}
fn persist(dir: &Directory, name: &str, value: &Value) -> Result<(), RunnerError> {
    let mut file = storage(dir.create_file(name))?;
    file.write_all(&serde_json::to_vec(value).unwrap())
        .and_then(|_| file.sync_all())
        .map_err(|_| RunnerError::from("runner_store_recovery_required"))?;
    storage(dir.sync())
}
fn decode(dir: &Directory, name: &str) -> Result<Value, RunnerError> {
    crate::request::parse_bounded_json(&storage(fs::read(dir, name, 65536))?, 12)
        .map_err(|_| "runner_store_recovery_required".into())
}
struct Namespace {
    path: PathBuf,
    owner: Value,
}
fn verify_namespace(namespace: &Namespace) -> Result<(), RunnerError> {
    let (parent, _) = storage(fs::open_root(
        namespace
            .path
            .parent()
            .ok_or(RunnerError::from("runner_namespace_mismatch"))?,
        false,
    ))?;
    let meta = parent
        .0
        .metadata()
        .map_err(|_| RunnerError::from("runner_namespace_mismatch"))?;
    if decode(&parent, "owner.json")? != namespace.owner
        || namespace.owner["device"] != meta.dev()
        || namespace.owner["inode"] != meta.ino()
    {
        return Err("runner_namespace_mismatch".into());
    }
    Ok(())
}
fn namespaces(
    cfg: &Server,
    c: &RunControl,
    d: Instant,
) -> Result<(std::fs::File, BTreeMap<String, Namespace>), RunnerError> {
    let (root, _) = storage(fs::open_root(Path::new(&cfg.state_dir), true))?;
    let names = storage(root.names(20))?;
    let empty = names.is_empty();
    let (lock, _) = storage(fs::lock(&root, empty))?;
    let metadata = root
        .0
        .metadata()
        .map_err(|_| RunnerError::from("runner_store_recovery_required"))?;
    let expected = json!({"format_version":1,"kind":"authenticated-runner","runner_id":cfg.runner_id,"uid":metadata.uid(),"device":metadata.dev(),"inode":metadata.ino()});
    if empty {
        persist(&root, "format.json", &expected)?;
    } else if decode(&root, "format.json")? != expected {
        return Err("runner_namespace_mismatch".into());
    }
    for name in &names {
        if name != "lock"
            && name != "format.json"
            && !(name.len() == 74
                && name.starts_with("principal-")
                && name[10..]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        {
            return Err("runner_store_recovery_required".into());
        }
    }
    let mut result = BTreeMap::new();
    for p in &cfg.principals {
        active(c, d)?;
        let name = format!("principal-{}", &hash(p.id.as_bytes())[7..]);
        let path = Path::new(&cfg.state_dir).join(&name);
        let existed = names.contains(&name);
        let namespace = if existed {
            storage(root.open_at(&name, true))?
        } else {
            storage(root.create_dir(&name))?
        };
        let nmeta = namespace
            .0
            .metadata()
            .map_err(|_| RunnerError::from("runner_store_recovery_required"))?;
        let owner = json!({"format_version":1,"principal":p.id,"runner_id":cfg.runner_id,"root":expected,"device":nmeta.dev(),"inode":nmeta.ino()});
        if existed {
            if decode(&namespace, "owner.json")? != owner {
                return Err("runner_namespace_mismatch".into());
            }
            let n = storage(namespace.names(3))?;
            if n.len() != 2 || !n.contains(&"owner.json".into()) || !n.contains(&"jobs".into()) {
                return Err("runner_store_recovery_required".into());
            }
        } else {
            persist(&namespace, "owner.json", &owner)?;
        }
        let jobs = path.join("jobs");
        crate::jobs::remote::initialize(&jobs, c, d).map_err(RunnerError::from)?;
        storage(namespace.sync())?;
        storage(root.sync())?;
        result.insert(p.id.clone(), Namespace { path: jobs, owner });
    }
    Ok((lock, result))
}
fn scope(
    cfg: &Server,
    policy: &str,
    principal: &str,
    target: &str,
    operation: &str,
) -> Option<Value> {
    let grant = cfg.grants.iter().find(|g| {
        g["principal"] == principal && g["target"] == target && g["operation"] == operation
    })?;
    let target = cfg.targets.iter().find(|t| t.id == target)?;
    Some(
        json!({"principal":principal,"runner_id":cfg.runner_id,"policy_sha256":policy,"target":{"id":target.id,"snapshot":target.snapshot,"snapshot_sha256":hash(&serde_json::to_vec(&target.snapshot).unwrap())},"grant":grant,"grant_sha256":hash(&serde_json::to_vec(grant).unwrap())}),
    )
}
fn base(
    cfg: &Server,
    policy: &str,
    cert: &str,
    principal: &config::Principal,
    method: &str,
    input: &Value,
) -> Value {
    json!({"format_version":1,"action":method,"admission":"not_requested","runner":{"id":cfg.runner_id,"protocol":1,"platform":"linux-x86_64","server_certificate_sha256":cert,"policy_sha256":policy},"principal":{"id":principal.id,"client_certificate_sha256":principal.certificate_sha256},"target":null,"request_id":input.get("request_id"),"request_sha256":null,"job":null,"events":[],"cursor":null,"result":null,"result_sha256":null,"result_omitted":false,"clock":{"runner_at":crate::result::timestamp(),"offset_established":false},"cancellation_signal":null,"capabilities":[],"limits":{"max_request_bytes":65536,"max_response_bytes":2097152,"max_runtime_ms":3600000,"max_workers":2,"max_connections":8},"remote_error":null,"admission_expires_at":null,"management_effect_outcome":"not_attempted","binding":null})
}
fn admission_state(method: &str, receipt: &OperationResult) -> &'static str {
    if receipt.data["not_found"] == true {
        "not_found"
    } else if !receipt.data["job"].is_null() {
        "accepted"
    } else if method == "submit" && receipt.effect_outcome == "unknown" {
        "unknown"
    } else {
        "not_requested"
    }
}
#[allow(
    clippy::too_many_arguments,
    reason = "Keep authenticated identity, namespace and request controls explicit at the private dispatch boundary"
)]
fn handle(
    cfg: &Server,
    policy: &str,
    cert: &str,
    principal: &config::Principal,
    namespace: &Namespace,
    wire: &Value,
    c: &RunControl,
    d: Instant,
) -> Value {
    let method = wire["method"].as_str().unwrap_or("");
    let input = &wire["inputs"];
    let mut data = base(cfg, policy, cert, principal, method, input);
    let outcome = (|| -> Result<(), RunnerError> {
        if !(keys(wire, &["protocol", "method", "inputs"])
            || keys(wire, &["protocol", "method", "inputs", "expected"]))
            || wire
                .get("expected")
                .is_some_and(|e| !super::valid_expected(e))
            || wire["protocol"] != 1
            || !super::input(method, input)
        {
            return Err("runner_invalid_protocol".into());
        }
        active(c, d)?;
        if let Some(expected) = wire.get("expected")
            && (expected["principal"] != principal.id
                || expected.get("policy_sha256").is_some_and(|p| p != policy))
        {
            return Err("runner_precondition_failed".into());
        }
        verify_namespace(namespace)?;
        let path = &namespace.path;
        if method == "handshake" {
            data["management_effect_outcome"] = json!("not_applicable");
            data["capabilities"]=json!(cfg.grants.iter().filter(|g|g["principal"]==principal.id).map(|g|json!({"target":g["target"],"operation":g["operation"],"version":1,"implementation_sha256":if g["operation"]=="task.run"{g["task"]["binding_sha256"].clone()}else{g["package_sha256"].clone()},"max_runtime_ms":g["max_runtime_ms"]})).collect::<Vec<_>>());
            return Ok(());
        }
        let context = cfg.context(path.to_path_buf());
        let receipt = if method == "submit" {
            let prior = crate::jobs::remote::lookup(
                path,
                input["request_id"].as_str().unwrap(),
                &principal.id,
                &cfg.runner_id,
                c,
                d,
            )
            .map_err(RunnerError::from)?;
            let admitted_scope = if let Some((_, binding)) = prior {
                binding["scope"].clone()
            } else {
                let s = scope(
                    cfg,
                    policy,
                    &principal.id,
                    input["target"].as_str().unwrap(),
                    input["operation"].as_str().unwrap(),
                )
                .ok_or(RunnerError::from("runner_grant_denied"))?;
                if input["max_runtime_ms"].as_u64().unwrap()
                    > s["grant"]["max_runtime_ms"].as_u64().unwrap()
                {
                    return Err("runner_runtime_denied".into());
                }
                s
            };
            crate::jobs::remote::submit(input, admitted_scope, &context, c, d)
        } else {
            crate::jobs::remote::observe(
                method,
                &principal.id,
                &cfg.runner_id,
                input,
                &context,
                c,
                d,
            )
        };
        data["management_effect_outcome"] = json!(receipt.effect_outcome);
        data["binding"] = receipt.data["remote_binding"].clone();
        for key in [
            "job",
            "events",
            "cursor",
            "result",
            "result_sha256",
            "result_omitted",
            "cancellation_signal",
        ] {
            data[key] = receipt.data[key].clone();
        }
        data["action"] = receipt.data["action"].clone();
        data["admission"] = json!(admission_state(method, &receipt));
        data["request_sha256"] = data["job"]["request_sha256"].clone();
        data["target"] = receipt.data["remote_binding"]["scope"]["target"].clone();
        data["admission_expires_at"] = receipt.data["remote_binding"]["expires_at"].clone();
        if receipt.execution.status != Status::Succeeded
            && receipt.execution.status != Status::Partial
        {
            data["remote_error"] = serde_json::to_value(&receipt.errors).unwrap();
            return Err(if receipt
                .errors
                .iter()
                .any(|e| e.code == "job_request_conflict")
            {
                "job_request_conflict"
            } else {
                "runner_job_refused"
            }
            .into());
        }
        Ok(())
    })();
    json!({"protocol":1,"ok":outcome.is_ok(),"error":outcome.err().map(|e|e.code),"data":data})
}
#[allow(
    clippy::too_many_arguments,
    reason = "Authenticated identity and request controls remain explicit"
)]
fn change_handle(
    cfg: &Server,
    policy: &str,
    cert: &str,
    principal: &config::Principal,
    changes: Option<&crate::change::Service>,
    wire: &Value,
    c: &RunControl,
    d: Instant,
) -> Value {
    let method = wire["method"].as_str().unwrap_or("");
    let request_id = wire["inputs"]["request_id"]
        .as_str()
        .filter(|s| crate::request::bounded(s, 1, 128) && !s.chars().any(char::is_control))
        .map(|s| json!(s))
        .unwrap_or(Value::Null);
    let error = if !(keys(wire, &["protocol", "method", "inputs"])
        || keys(wire, &["protocol", "method", "inputs", "expected"]))
        || wire["protocol"] != 1
        || wire
            .get("expected")
            .is_some_and(|e| !super::valid_expected(e))
    {
        Some("runner_invalid_protocol")
    } else if wire.get("expected").is_some_and(|e| {
        e["principal"] != principal.id || e.get("policy_sha256").is_some_and(|p| p != policy)
    }) {
        Some("runner_precondition_failed")
    } else {
        None
    };
    if let Some(service) = changes {
        if let Some(error) = error {
            let mut response = service.handle(
                &principal.id,
                &principal.certificate_sha256,
                method,
                &Value::Null,
                c,
                d,
            );
            response["error"] = json!(error);
            response["data"]["remote_error"] = json!(error);
            response["data"]["request_id"] = request_id;
            return response;
        }
        return service.handle(
            &principal.id,
            &principal.certificate_sha256,
            method,
            &wire["inputs"],
            c,
            d,
        );
    }
    let error = error.unwrap_or("change_disabled");
    json!({"protocol":1,"ok":false,"error":error,"data":{"format_version":1,"action":method.strip_prefix("change.").unwrap_or(method),"runner":{"id":cfg.runner_id,"server_certificate_sha256":cert,"policy_sha256":policy},"principal":{"id":principal.id,"client_certificate_sha256":principal.certificate_sha256},"requester":null,"request_id":request_id,"target":null,"plan":null,"plan_sha256":null,"approval_id":null,"state":"not_found","effect_outcome":"not_attempted","remote_error":error}})
}
pub(super) fn serve(
    path: &Path,
    control: &RunControl,
    ready: impl FnOnce(Value) -> Result<(), ()>,
) -> Result<(), RunnerError> {
    if !cfg!(target_arch = "x86_64") {
        return Err("runner_platform_unsupported".into());
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    let (cfg, policy) = config::server(path, control, deadline)?;
    let material = cfg.material(control, deadline)?;
    let cert = hash(&material.certificate);
    let changes = if cfg.changes.is_some() {
        Some(
            crate::change::Service::new(
                &serde_json::to_value(&cfg)
                    .map_err(|_| RunnerError::from("invalid_runner_config"))?,
                &cfg.runner_id,
                &cert,
                &policy,
                cfg.promtool_path.as_deref().map(Path::new),
            )
            .map_err(RunnerError::from)?,
        )
    } else {
        None
    };
    let tls = transport::server_config(material).map_err(RunnerError::from)?;
    let after_effects = |mut error: RunnerError| {
        error.effects = true;
        error
    };
    let (_lock, paths) = namespaces(&cfg, control, deadline).map_err(after_effects)?;
    let listener = TcpListener::bind(&cfg.listen)
        .map_err(|_| after_effects(RunnerError::from("runner_bind_failed")))?;
    listener
        .set_nonblocking(true)
        .map_err(|_| after_effects(RunnerError::from("runner_bind_failed")))?;
    let address = listener
        .local_addr()
        .map_err(|_| after_effects(RunnerError::from("runner_bind_failed")))?
        .to_string();
    ready(json!({"runner_id":cfg.runner_id,"protocol":1,"address":address,"server_certificate_sha256":cert})).map_err(|_|after_effects(RunnerError::from("runner_ready_consumer_closed")))?;
    let worker_stop = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    // Scoped workers never outlive this explicit server. Only signals and durable cancel
    // intents control children; disconnected transport consumers do not.
    std::thread::scope(|threads| {
        let mut connections: Vec<(std::thread::ScopedJoinHandle<'_, ()>, std::net::TcpStream)> =
            vec![];
        let mut workers: Vec<(String, std::thread::ScopedJoinHandle<'_, ()>)> = vec![];
        let mut failed = None;
        while control.signal.load(Ordering::Relaxed) == 0 {
            let mut i = 0;
            while i < connections.len() {
                if connections[i].0.is_finished() {
                    let _ = connections.swap_remove(i).0.join();
                } else {
                    i += 1
                }
            }
            let mut i = 0;
            while i < workers.len() {
                if workers[i].1.is_finished() {
                    let _ = workers.swap_remove(i).1.join();
                } else {
                    i += 1
                }
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    if connections.len() < 8 {
                        let tls = tls.clone();
                        let cfg = &cfg;
                        let policy = &policy;
                        let cert = &cert;
                        let paths = &paths;
                        let changes = &changes;
                        let Ok(shutdown) = stream.try_clone() else {
                            continue;
                        };
                        connections.push((
                            threads.spawn(move || {
                                let d = Instant::now() + Duration::from_secs(30);
                                let Ok(mut channel) = transport::accept(stream, tls, d) else {
                                    return;
                                };
                                let Some(principal) = cfg
                                    .principals
                                    .iter()
                                    .find(|p| p.certificate_sha256 == channel.peer_sha256())
                                else {
                                    return;
                                };
                                let Ok(bytes) = channel.read_request() else {
                                    return;
                                };
                                let Ok(wire) = crate::request::parse_bounded_json(&bytes, 20)
                                else {
                                    return;
                                };
                                let method = wire["method"].as_str().unwrap_or("");
                                let response = if method.starts_with("change.") {
                                    change_handle(
                                        cfg,
                                        policy,
                                        cert,
                                        principal,
                                        changes.as_ref(),
                                        &wire,
                                        control,
                                        d,
                                    )
                                } else {
                                    handle(
                                        cfg,
                                        policy,
                                        cert,
                                        principal,
                                        &paths[&principal.id],
                                        &wire,
                                        control,
                                        d,
                                    )
                                };
                                let bytes = serde_json::to_vec(&response).unwrap();
                                if bytes.len() <= 2 * 1024 * 1024 {
                                    let _ = channel.write_response(&bytes);
                                }
                            }),
                            shutdown,
                        ));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {
                    failed = Some("runner_accept_failed");
                    break;
                }
            }
            for (principal, namespace) in &paths {
                if verify_namespace(namespace).is_err() {
                    failed = Some("runner_namespace_mismatch");
                    break;
                }
                let path = &namespace.path;
                if workers.len() >= 2 {
                    break;
                }
                let scan = crate::jobs::remote::pending(
                    path,
                    principal,
                    &cfg.runner_id,
                    control,
                    Instant::now() + Duration::from_millis(100),
                );
                let pending = match scan {
                    Ok(p) => p,
                    Err("job_store_busy" | "job_deadline_exceeded") => continue,
                    Err("job_cancelled") if control.signal.load(Ordering::Relaxed) != 0 => break,
                    Err(_) => {
                        failed = Some("runner_store_recovery_required");
                        break;
                    }
                };
                for (id, binding) in pending {
                    if workers.len() >= 2 {
                        break;
                    }
                    let key = format!("{principal}:{id}");
                    if workers.iter().any(|(key2, _)| key2 == &key) {
                        continue;
                    }
                    let current = scope(
                        &cfg,
                        &policy,
                        principal,
                        binding["scope"]["target"]["id"].as_str().unwrap_or(""),
                        binding["scope"]["grant"]["operation"]
                            .as_str()
                            .unwrap_or(""),
                    )
                    .unwrap_or(Value::Null);
                    let context = cfg.context(path.clone());
                    let stop = worker_stop.clone();
                    workers.push((
                        key,
                        threads.spawn(move || {
                            let c = RunControl {
                                signal: stop,
                                result_fd: None,
                            };
                            let _ = crate::jobs::remote::work(&id, &current, &context, &c);
                        }),
                    ));
                }
            }
            if failed.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let external_signal = control.signal.load(Ordering::Relaxed);
        worker_stop.store(
            if external_signal != 0 {
                external_signal
            } else {
                15
            },
            Ordering::Relaxed,
        );
        for (_, socket) in &connections {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
        for (_, worker) in workers {
            let _ = worker.join();
        }
        for (connection, _) in connections {
            let _ = connection.join();
        }
        failed.map_or(Ok(()), |e| Err(after_effects(e.into())))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_change_refusal_keeps_bounded_request_identity() {
        let cfg:Server=serde_json::from_value(json!({"config_version":1,"runner_id":"runner","listen":"127.0.0.1:0","state_dir":"/unused","ca_certificate":"/unused","certificate":"/unused","private_key":"/unused","principals":[{"id":"alice","certificate_sha256":hash(b"alice")}],"targets":[],"grants":[]})).unwrap();
        let wire = json!({"protocol":1,"method":"change.apply","inputs":{"request_id":"known"}});
        let response = change_handle(
            &cfg,
            &hash(b"policy"),
            &hash(b"server"),
            &cfg.principals[0],
            None,
            &wire,
            &RunControl::default(),
            Instant::now() + Duration::from_secs(1),
        );
        assert_eq!(response["error"], "change_disabled");
        assert_eq!(response["data"]["request_id"], "known");
        assert_eq!(response["data"]["effect_outcome"], "not_attempted");
    }
    #[test]
    fn namespace_binding_detects_replacement_and_preserves_unknown_layout() {
        let root = std::env::temp_dir().join(crate::result::new_id("runner-namespace-test"));
        let cfg:Server=serde_json::from_value(json!({"config_version":1,"runner_id":"runner","listen":"127.0.0.1:0","state_dir":root,"ca_certificate":"/unused","certificate":"/unused","private_key":"/unused","principals":[{"id":"alice","certificate_sha256":format!("sha256:{}","a".repeat(64))}],"targets":[{"id":"test","snapshot":{"name":"test","environment":"test"}}],"grants":[]})).unwrap();
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(2);
        let (lock, map) = namespaces(&cfg, &c, d).unwrap();
        verify_namespace(&map["alice"]).unwrap();
        let owner = map["alice"].path.parent().unwrap().join("owner.json");
        std::fs::write(&owner, b"{}").unwrap();
        assert_eq!(
            verify_namespace(&map["alice"]).unwrap_err().code,
            "runner_namespace_mismatch"
        );
        drop(lock);
        assert_eq!(
            namespaces(&cfg, &c, d).err().unwrap().code,
            "runner_namespace_mismatch"
        );
        assert_eq!(std::fs::read(owner).unwrap(), b"{}");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn real_durable_response_rejects_mutated_nested_receipts_and_bindings() {
        let root = std::env::temp_dir().join(crate::result::new_id("runner-response-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(10);
        let task = crate::Request::new("task.describe", json!({"id":"error-budget"}));
        let mut described = OperationResult::new(&task);
        crate::tasks::describe(&task.inputs, &mut described);
        let digest = format!("sha256:{}", "a".repeat(64));
        let cfg:Server=serde_json::from_value(json!({"config_version":1,"runner_id":"runner","listen":"127.0.0.1:0","state_dir":root,"ca_certificate":"/unused","certificate":"/unused","private_key":"/unused","principals":[{"id":"alice","certificate_sha256":digest}],"targets":[{"id":"test","snapshot":{"name":"test","environment":"test"}}],"grants":[{"principal":"alice","target":"test","operation":"task.run","operation_version":1,"task":{"id":"error-budget","version":1,"binding_sha256":described.data["source"]["binding_digest"]},"max_runtime_ms":1000}]})).unwrap();
        let (lock, map) = namespaces(&cfg, &c, d).unwrap();
        let namespace = &map["alice"];
        let wire = json!({"protocol":1,"method":"submit","inputs":{"request_id":"known","target":"test","operation":"task.run","operation_version":1,"inputs":{"id":"error-budget","version":1,"input":{"slo":99.9,"bad_minutes":1}},"max_runtime_ms":1000,"retain_events":2}});
        let submitted = handle(
            &cfg,
            &digest,
            &digest,
            &cfg.principals[0],
            namespace,
            &wire,
            &c,
            d,
        );
        assert_eq!(submitted["ok"], true, "{submitted}");
        assert!(super::super::response::valid(&submitted), "{submitted}");
        let cancel = json!({"protocol":1,"method":"cancel","inputs":{"request_id":"known"}});
        let response = handle(
            &cfg,
            &digest,
            &digest,
            &cfg.principals[0],
            namespace,
            &cancel,
            &c,
            d,
        );
        assert!(super::super::response::valid(&response), "{response}");
        for pointer in [
            "/data/management_effect_outcome",
            "/data/result/effect_outcome",
            "/data/result/execution/status",
            "/data/events",
            "/data/binding/scope/principal",
        ] {
            let mut bad = response.clone();
            *bad.pointer_mut(pointer).unwrap() = json!("bogus");
            assert!(!super::super::response::valid(&bad), "{pointer}");
        }
        drop(lock);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unconfirmed_submit_writes_keep_admission_unknown_without_a_job_handle() {
        let request = Request::new("job.submit", json!({}));
        let mut receipt = OperationResult::new(&request);
        receipt.data = json!({"job":null});
        receipt.execution.status = Status::Failed;
        receipt.effect_outcome = "unknown".into();
        assert_eq!(admission_state("submit", &receipt), "unknown");
        receipt.effect_outcome = "not_attempted".into();
        assert_eq!(admission_state("submit", &receipt), "not_requested");
        receipt.data["job"] = json!({"job_id":"already-observed"});
        receipt.effect_outcome = "unknown".into();
        assert_eq!(admission_state("submit", &receipt), "accepted");
    }
}
