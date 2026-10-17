use super::*;
use crate::evidence::store::{
    StoreError,
    fs::{self, Directory},
};
use model::{Admission, State};
use std::{
    fs::File,
    io::Write,
    os::fd::AsRawFd,
    path::Path,
    sync::{Arc, atomic::AtomicUsize},
};
const MAX_RUNS: usize = 32;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;
const RESERVED: u64 = 1920 * 1024;
const MAX_ADMISSION: usize = 64 * 1024;
const MAX_STATE: usize = 576 * 1024;
const MAX_EVIDENCE: usize = 32 * 1024;
fn converted(e: StoreError) -> Fault {
    Fault {
        code: match e.code {
            "store_not_found" => "fleet_store_not_found",
            "store_busy" => "fleet_store_busy",
            "store_no_space" => "fleet_store_no_space",
            _ => "fleet_store_recovery_required",
        },
        effects: e.effects,
    }
}
pub(super) struct Store {
    root: Directory,
    _lock: File,
    deadline: Instant,
    signal: Arc<AtomicUsize>,
    pub created: bool,
}
pub(super) struct Run {
    pub admission: Admission,
    pub state: State,
    pub durable_state: State,
    pub directory: Directory,
}
impl Store {
    pub fn open(path: &Path, create: bool, c: &RunControl, d: Instant) -> Result<Self, Fault> {
        active(c, d)?;
        if !path.is_absolute()
            || path
                .to_str()
                .is_none_or(|s| s.len() > 4096 || s.chars().any(char::is_control))
        {
            return Err(Fault::new("invalid_fleet_store_path"));
        }
        let (root, created) = fs::open_root(path, create).map_err(converted)?;
        let opened = (|| {
            let names = root.names(MAX_RUNS + 2).map_err(converted)?;
            let initialize = create && names.is_empty();
            if !initialize
                && (!names.iter().any(|s| s == "format.json") || !names.iter().any(|s| s == "lock"))
            {
                return Err(Fault::new("fleet_store_recovery_required"));
            }
            let (lock, lock_created) = loop {
                active(c, d)?;
                match fs::lock(&root, initialize) {
                    Ok(v) => break v,
                    Err(e) if e.code == "store_busy" => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(e) => return Err(converted(e)),
                }
            };
            let finish = (|| {
                use std::os::unix::fs::MetadataExt;
                let metadata = root
                    .0
                    .metadata()
                    .map_err(|_| Fault::new("fleet_store_io"))?;
                let expected = json!({"format_version":1,"kind":"fleet-observations","owner_uid":model::uid(),"device":metadata.dev(),"inode":metadata.ino()});
                if initialize {
                    if !lock_created || root.names(2).map_err(converted)? != ["lock"] {
                        return Err(Fault::new("fleet_store_recovery_required"));
                    }
                    publish(
                        &root,
                        "format.json",
                        &serde_json::to_vec(&expected).unwrap(),
                        false,
                        c,
                        d,
                    )?;
                }
                let format = parse(&fs::read(&root, "format.json", 1024).map_err(converted)?, 8)?;
                if format != expected {
                    return Err(Fault::new("fleet_store_owner_mismatch"));
                }
                Ok(())
            })();
            finish.map_err(|e| {
                if created || lock_created {
                    e.after_effects()
                } else {
                    e
                }
            })?;
            Ok((lock, created || lock_created))
        })();
        let (lock, wrote) =
            opened.map_err(|e: Fault| if created { e.after_effects() } else { e })?;
        let store = Self {
            root,
            _lock: lock,
            deadline: d,
            signal: c.signal.clone(),
            created: wrote,
        };
        store
            .scan()
            .map_err(|e| if store.created { e.after_effects() } else { e })?;
        Ok(store)
    }
    pub fn identity(&self) -> Result<Value, Fault> {
        use std::os::unix::fs::MetadataExt;
        let m = self
            .root
            .0
            .metadata()
            .map_err(|_| Fault::new("fleet_store_io"))?;
        Ok(json!({"owner_uid":model::uid(),"device":m.dev(),"inode":m.ino()}))
    }
    fn control(&self) -> RunControl {
        RunControl {
            signal: self.signal.clone(),
            result_fd: None,
        }
    }
    fn scan(&self) -> Result<Vec<String>, Fault> {
        let mut ids = vec![];
        let mut total = 0;
        for name in self.root.names(MAX_RUNS + 2).map_err(converted)? {
            active(&self.control(), self.deadline)?;
            if matches!(name.as_str(), "lock" | "format.json") {
                total += self
                    .root
                    .open_file(&name, false)
                    .map_err(converted)?
                    .metadata()
                    .map_err(|_| Fault::new("fleet_store_io"))?
                    .len();
                continue;
            }
            if !model::valid_id(&name) {
                return Err(Fault::new("fleet_store_recovery_required"));
            }
            let directory = self.root.open_at(&name, true).map_err(converted)?;
            let files = directory.names(19).map_err(converted)?;
            if !["admission.json", "state.json", "lease"]
                .iter()
                .all(|n| files.iter().any(|f| f == n))
            {
                return Err(Fault::new("fleet_store_recovery_required"));
            }
            for file in files {
                let maximum = match file.as_str() {
                    "admission.json" => MAX_ADMISSION,
                    "state.json" => MAX_STATE,
                    "lease" => 0,
                    name if evidence_index(name).is_some() => MAX_EVIDENCE,
                    _ => return Err(Fault::new("fleet_store_recovery_required")),
                };
                let n = directory
                    .open_file(&file, false)
                    .map_err(converted)?
                    .metadata()
                    .map_err(|_| Fault::new("fleet_store_io"))?
                    .len();
                if n > maximum as u64 {
                    return Err(Fault::new("fleet_store_recovery_required"));
                }
                total += n;
            }
            ids.push(name);
        }
        if ids.len() > MAX_RUNS || total > MAX_TOTAL {
            return Err(Fault::new("fleet_quota_exceeded"));
        }
        Ok(ids)
    }
    pub fn ids(&self) -> Result<Vec<String>, Fault> {
        self.scan()
    }
    pub fn load(&self, id: &str) -> Result<Run, Fault> {
        if !model::valid_id(id) {
            return Err(Fault::new("invalid_fleet_input"));
        }
        let directory = self.root.open_at(id, true).map_err(converted)?;
        let value = parse(
            &fs::read(&directory, "admission.json", MAX_ADMISSION).map_err(converted)?,
            24,
        )?;
        if !value.is_object()
            || !value["targets"]
                .as_array()
                .is_some_and(|a| a.iter().all(|t| t.is_object() && t["entry"].is_object()))
        {
            return Err(Fault::new("fleet_store_recovery_required"));
        }
        let admission: Admission = serde_json::from_value(value)
            .map_err(|_| Fault::new("fleet_store_recovery_required"))?;
        let value = parse(
            &fs::read(&directory, "state.json", MAX_STATE).map_err(converted)?,
            72,
        )?;
        if !value.is_object()
            || !value["targets"].as_array().is_some_and(|a| {
                a.iter().all(|t| {
                    t.is_object() && (t["observation"].is_null() || t["observation"].is_object())
                })
            })
        {
            return Err(Fault::new("fleet_store_recovery_required"));
        }
        let state: State = serde_json::from_value(value)
            .map_err(|_| Fault::new("fleet_store_recovery_required"))?;
        if !admission.valid(id) || !state.valid(&admission) {
            return Err(Fault::new("fleet_store_recovery_required"));
        }
        let run = Run {
            admission,
            durable_state: state.clone(),
            state,
            directory,
        };
        for file in run.directory.names(19).map_err(converted)? {
            if let Some(index) = evidence_index(&file) {
                if index >= run.state.targets.len() {
                    return Err(Fault::new("fleet_store_recovery_required"));
                }
                self.verify_evidence(&run, index)?;
            }
        }
        for (index, target) in run.state.targets.iter().enumerate() {
            if target.phase == "terminal" {
                self.verify_evidence(&run, index)?;
            }
        }
        Ok(run)
    }
    pub fn find(&self, request_id: &str) -> Result<Option<Run>, Fault> {
        let mut found = None;
        for id in self.ids()? {
            let run = self.load(&id)?;
            if run.admission.request_id == request_id {
                if found.is_some() {
                    return Err(Fault::new("fleet_store_recovery_required"));
                }
                found = Some(run);
            }
        }
        Ok(found)
    }
    pub fn ordinal(&self, selection: &str) -> Result<u64, Fault> {
        let mut n = 0;
        for id in self.ids()? {
            let a = self.load(&id)?.admission;
            if a.selection_sha256 == selection {
                n = n.max(
                    a.ordinal
                        .checked_add(1)
                        .ok_or(Fault::new("fleet_quota_exceeded"))?,
                );
            }
        }
        if n >= 9_007_199_254_740_991 {
            return Err(Fault::new("fleet_quota_exceeded"));
        }
        Ok(n)
    }
    pub fn create(&self, a: Admission) -> Result<Run, Fault> {
        let ids = self.ids()?;
        if ids.len() >= MAX_RUNS || (ids.len() as u64 + 1) * RESERVED + 4096 > MAX_TOTAL {
            return Err(Fault::new("fleet_quota_exceeded"));
        }
        if !a.valid(&a.id) {
            return Err(Fault::new("fleet_invalid_admission"));
        }
        let state = State::new(&a);
        let bytes = serde_json::to_vec(&a).unwrap();
        if bytes.len() > MAX_ADMISSION {
            return Err(Fault::new("fleet_quota_exceeded"));
        }
        active(&self.control(), self.deadline)?;
        let directory = self.root.create_dir(&a.id).map_err(converted)?;
        let result = (|| {
            directory
                .create_file("lease")
                .map_err(converted)?
                .sync_all()
                .map_err(|_| Fault::new("fleet_store_io"))?;
            publish(
                &directory,
                "admission.json",
                &bytes,
                false,
                &self.control(),
                self.deadline,
            )?;
            publish(
                &directory,
                "state.json",
                &serde_json::to_vec(&state).unwrap(),
                false,
                &self.control(),
                self.deadline,
            )?;
            self.root.sync().map_err(converted)?;
            Ok(())
        })();
        result.map_err(Fault::after_effects)?;
        Ok(Run {
            admission: a,
            durable_state: state.clone(),
            state,
            directory,
        })
    }
    pub fn commit(&self, run: &mut Run) -> Result<(), Fault> {
        if !run.state.valid(&run.admission) {
            return Err(Fault::new("fleet_invalid_transition"));
        }
        let bytes = serde_json::to_vec(&run.state).unwrap();
        if bytes.len() > MAX_STATE {
            return Err(Fault::new("fleet_quota_exceeded"));
        }
        let previous = parse(
            &fs::read(&run.directory, "state.json", MAX_STATE).map_err(converted)?,
            72,
        )?;
        if previous["phase"] == "terminal" {
            return Err(Fault::new("fleet_terminal_immutable"));
        }
        publish(
            &run.directory,
            "state.json",
            &bytes,
            true,
            &self.control(),
            self.deadline,
        )?;
        run.durable_state = run.state.clone();
        Ok(())
    }
    pub fn finalize_target(&self, run: &mut Run, index: usize) -> Result<(), Fault> {
        let bytes = serde_json::to_vec(&model::evidence(
            &run.admission,
            index,
            &run.state.targets[index],
        ))
        .unwrap();
        if bytes.len() > MAX_EVIDENCE {
            return Err(Fault::new("fleet_quota_exceeded"));
        }
        let digest = hash(&bytes);
        if run.state.targets[index]
            .evidence_sha256
            .as_ref()
            .is_some_and(|d| d != &digest)
        {
            return Err(Fault::new("fleet_store_recovery_required"));
        }
        run.state.targets[index].evidence_sha256 = Some(digest);
        self.commit(run)?;
        let outcome = (|| {
            let name = format!("target-{index:02}.json");
            let names = run.directory.names(19).map_err(converted)?;
            if names.contains(&name) {
                self.verify_evidence(run, index)?;
            } else {
                publish(
                    &run.directory,
                    &name,
                    &bytes,
                    false,
                    &self.control(),
                    self.deadline,
                )?;
            }
            run.state.targets[index].phase = "terminal".into();
            self.commit(run)
        })();
        outcome.map_err(Fault::after_effects)
    }
    fn verify_evidence(&self, run: &Run, index: usize) -> Result<(), Fault> {
        let bytes = fs::read(
            &run.directory,
            &format!("target-{index:02}.json"),
            MAX_EVIDENCE,
        )
        .map_err(converted)?;
        let state = &run.state.targets[index];
        if !matches!(state.phase.as_str(), "finalizing" | "terminal")
            || state.evidence_sha256.as_deref() != Some(&hash(&bytes))
            || parse(&bytes, 72)? != model::evidence(&run.admission, index, state)
        {
            return Err(Fault::new("fleet_store_recovery_required"));
        }
        Ok(())
    }
}
pub(super) fn claim(run: &Run) -> Result<Option<File>, Fault> {
    let file = run.directory.open_file("lease", true).map_err(converted)?; // SAFETY: a live independently opened descriptor receives a nonblocking advisory lock.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        Ok(Some(file))
    } else if std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(None)
    } else {
        Err(Fault::new("fleet_store_io"))
    }
}
fn parse(bytes: &[u8], depth: usize) -> Result<Value, Fault> {
    crate::request::parse_bounded_json(bytes, depth)
        .map_err(|_| Fault::new("fleet_store_recovery_required"))
}
fn evidence_index(name: &str) -> Option<usize> {
    let n = name.strip_prefix("target-")?.strip_suffix(".json")?;
    if n.len() != 2 {
        return None;
    }
    let n: usize = n.parse().ok()?;
    (n < 16).then_some(n)
}
fn publish(
    dir: &Directory,
    name: &str,
    bytes: &[u8],
    replace: bool,
    c: &RunControl,
    d: Instant,
) -> Result<(), Fault> {
    active(c, d)?;
    let temporary = crate::result::new_id("pending");
    let mut file = dir.create_file(&temporary).map_err(converted)?;
    let outcome = (|| {
        for chunk in bytes.chunks(32768) {
            active(c, d)?;
            file.write_all(chunk)
                .map_err(|_| Fault::new("fleet_store_io"))?;
        }
        file.sync_all().map_err(|_| Fault::new("fleet_store_io"))?;
        active(c, d)?;
        #[cfg(test)]
        if FAIL_BEFORE_PUBLISH.with(|flag| flag.replace(false)) {
            return Err(Fault::new("fleet_test_write_failed"));
        }
        dir.publish(&temporary, name, replace).map_err(converted)?;
        dir.sync().map_err(converted)
    })();
    outcome.map_err(Fault::after_effects)
}
#[cfg(test)]
thread_local! {pub(super)static FAIL_BEFORE_PUBLISH:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persisted_phase_and_evidence_corruption_are_refused() {
        let path = std::env::temp_dir().join(crate::result::new_id("fleet-store-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(5);
        let store = Store::open(&path, true, &c, d).unwrap();
        let mut a = model::fixture();
        a.targets[0].preflight_error = Some("synthetic_preflight_failure".into());
        a.request_sha256 = a.fingerprint();
        let mut run = store.create(a).unwrap();
        store.finalize_target(&mut run, 0).unwrap();
        assert_eq!(
            store.load(&run.admission.id).unwrap().state.targets[0].phase,
            "terminal"
        );
        let evidence = path.join(&run.admission.id).join("target-00.json");
        let mut bytes = std::fs::read(&evidence).unwrap();
        bytes.push(b' ');
        std::fs::write(&evidence, &bytes).unwrap();
        assert_eq!(
            store.load(&run.admission.id).err().unwrap().code,
            "fleet_store_recovery_required"
        );
        assert_eq!(std::fs::read(evidence).unwrap(), bytes);
        drop(store);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn interrupted_publication_preserves_unknown_layout_and_effect_truth() {
        let path = std::env::temp_dir().join(crate::result::new_id("fleet-write-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(5);
        let store = Store::open(&path, true, &c, d).unwrap();
        FAIL_BEFORE_PUBLISH.with(|f| f.set(true));
        let error = store.create(model::fixture()).err().unwrap();
        assert!(error.effects);
        drop(store);
        assert_eq!(
            Store::open(&path, false, &c, d).err().unwrap().code,
            "fleet_store_recovery_required"
        );
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn malformed_state_is_not_returned_as_successful_public_metadata() {
        let path = std::env::temp_dir().join(crate::result::new_id("fleet-state-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(5);
        let store = Store::open(&path, true, &c, d).unwrap();
        let run = store.create(model::fixture()).unwrap();
        let file = path.join(&run.admission.id).join("state.json");
        let mut state = serde_json::to_value(&run.state).unwrap();
        state["updated_at"] = json!("invalid");
        std::fs::write(&file, serde_json::to_vec(&state).unwrap()).unwrap();
        assert_eq!(
            store.load(&run.admission.id).err().unwrap().code,
            "fleet_store_recovery_required"
        );
        drop(store);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn retained_receipt_mutation_with_original_digest_is_rejected_before_publication() {
        let path = std::env::temp_dir().join(crate::result::new_id("fleet-receipt-test"));
        let c = RunControl::default();
        let d = Instant::now() + Duration::from_secs(5);
        let store = Store::open(&path, true, &c, d).unwrap();
        let mut run = store.create(model::fixture()).unwrap();
        let receipt = serde_json::to_value(OperationResult::new(&Request::new(
            "runner.status",
            json!({}),
        )))
        .unwrap();
        run.state.targets[0].phase = "finalizing".into();
        run.state.targets[0].outcome = Some("failed".into());
        run.state.targets[0].observation = Some(model::Observation {
            at: crate::result::timestamp(),
            admission: "accepted".into(),
            remote_state: Some("terminal".into()),
            job_id: Some("known-job".into()),
            attempt_id: None,
            result_sha256: None,
            receipt: Some(receipt.clone()),
            receipt_sha256: model::digest(&receipt),
            receipt_omitted: false,
            error: None,
        });
        store.commit(&mut run).unwrap();
        assert!(store.load(&run.admission.id).is_ok());
        let file = path.join(&run.admission.id).join("state.json");
        let mut value = serde_json::to_value(&run.state).unwrap();
        value["targets"][0]["observation"]["receipt"]["execution"]["status"] = json!("failed");
        let bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(&file, &bytes).unwrap();
        assert_eq!(
            store.load(&run.admission.id).err().unwrap().code,
            "fleet_store_recovery_required"
        );
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        assert!(!path.join(&run.admission.id).join("target-00.json").exists());
        drop(store);
        std::fs::remove_dir_all(path).unwrap();
    }
}
