use super::*;
use crate::evidence::store::{
    StoreError,
    fs::{self, Directory},
};
use std::{
    fs::File,
    io::Write,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
};
const MAX_STATE: usize = 31 * 1024 * 1024;
fn converted(e: StoreError) -> Fault {
    Fault {
        code: match e.code {
            "store_not_found" => "schedule_store_not_found",
            "store_busy" => "schedule_store_busy",
            "store_no_space" => "schedule_store_no_space",
            _ => "schedule_store_recovery_required",
        },
        effects: e.effects,
    }
}
pub(super) struct Store {
    root: Directory,
    _lock: File,
    pub created: bool,
}
impl Store {
    pub fn open(path: &Path, create: bool, c: &RunControl, d: Instant) -> Result<Self, Fault> {
        active(c, d)?;
        if !path.is_absolute() {
            return Err(Fault::new("invalid_schedule_store_path"));
        }
        let (root, created) = fs::open_root(path, create).map_err(converted)?;
        let opened = (|| {
            let names = root.names(4).map_err(converted)?;
            let initialize = create && names.is_empty();
            if !initialize && names.len() != 4 {
                return Err(Fault::new("schedule_store_recovery_required"));
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
                let m = root
                    .0
                    .metadata()
                    .map_err(|_| Fault::new("schedule_store_io"))?;
                let expected = json!({"format_version":1,"kind":"observational-schedules","owner_uid":model::uid(),"device":m.dev(),"inode":m.ino()});
                if initialize {
                    if !lock_created || root.names(2).map_err(converted)? != ["lock"] {
                        return Err(Fault::new("schedule_store_recovery_required"));
                    }
                    root.create_file("lease")
                        .map_err(converted)?
                        .sync_all()
                        .map_err(|_| Fault::new("schedule_store_io"))?;
                    publish(&root, "format.json", &expected, false, c, d)?;
                    publish(
                        &root,
                        "state.json",
                        &serde_json::to_value(model::State::new()).unwrap(),
                        false,
                        c,
                        d,
                    )?;
                }
                let names = root.names(4).map_err(converted)?;
                if names.len() != 4
                    || names.iter().any(|n| {
                        !matches!(n.as_str(), "lock" | "lease" | "format.json" | "state.json")
                    })
                {
                    return Err(Fault::new("schedule_store_recovery_required"));
                }
                if parse(&fs::read(&root, "format.json", 1024).map_err(converted)?)? != expected
                    || root
                        .open_file("lease", false)
                        .map_err(converted)?
                        .metadata()
                        .map_err(|_| Fault::new("schedule_store_io"))?
                        .len()
                        != 0
                {
                    return Err(Fault::new("schedule_store_recovery_required"));
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
            created: wrote,
        };
        store
            .load()
            .map_err(|e| if wrote { e.after_effects() } else { e })?;
        Ok(store)
    }
    pub fn load(&self) -> Result<model::State, Fault> {
        let v = parse(&fs::read(&self.root, "state.json", MAX_STATE).map_err(converted)?)?;
        if !objects(&v) {
            return Err(Fault::new("schedule_store_recovery_required"));
        }
        let state: model::State = serde_json::from_value(v)
            .map_err(|_| Fault::new("schedule_store_recovery_required"))?;
        if !state.valid() {
            return Err(Fault::new("schedule_store_recovery_required"));
        }
        Ok(state)
    }
    pub fn commit(&self, state: &model::State, c: &RunControl, d: Instant) -> Result<(), Fault> {
        if !state.valid() {
            return Err(Fault::new("schedule_invalid_transition"));
        }
        if reserved_size(state).is_none_or(|n| n > MAX_STATE) {
            return Err(Fault::new("schedule_quota_exceeded"));
        }
        let old = self.load()?;
        if state.work_sequence < old.work_sequence {
            return Err(Fault::new("schedule_invalid_transition"));
        }
        for prior in &old.schedules {
            let now = state
                .schedules
                .iter()
                .find(|s| s.id == prior.id)
                .ok_or(Fault::new("schedule_invalid_transition"))?;
            if now.instance_id != prior.instance_id
                || now.revision < prior.revision
                || now.generation < prior.generation
                || now.generations.len() < prior.generations.len()
                || !prior
                    .generations
                    .iter()
                    .zip(&now.generations)
                    .all(|(a, b)| a.binding_sha256 == b.binding_sha256)
            {
                return Err(Fault::new("schedule_invalid_transition"));
            }
            for o in &prior.occurrences {
                let next = now
                    .occurrences
                    .iter()
                    .find(|n| n.id == o.id)
                    .ok_or(Fault::new("schedule_invalid_transition"))?;
                if o.resolved()
                    && serde_json::to_value(o).unwrap() != serde_json::to_value(next).unwrap()
                {
                    return Err(Fault::new("schedule_terminal_immutable"));
                }
                if o.unresolved()
                    && (next.state == "queued"
                        || next.state == "skipped"
                        || next.effective_timeout_ms != o.effective_timeout_ms)
                {
                    return Err(Fault::new("schedule_invalid_transition"));
                }
            }
        }
        publish(
            &self.root,
            "state.json",
            &serde_json::to_value(state).unwrap(),
            true,
            c,
            d,
        )
    }
    pub fn claim(&self) -> Result<File, Fault> {
        let file = self.root.open_file("lease", true).map_err(converted)?; // SAFETY: lock applies only to the live owned descriptor.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(Fault::new("schedule_worker_busy"));
        }
        Ok(file)
    }
}
// Each admitted row owns both its full receipt and all future metadata. Header
// headroom covers cursor/skip timestamps and monotonic counter digit growth.
pub(super) fn reserved_size(state: &model::State) -> Option<usize> {
    let mut bytes = serde_json::to_vec(state)
        .ok()?
        .len()
        .checked_add(16 * 1024)?;
    for o in state.schedules.iter().flat_map(|s| &s.occurrences) {
        let size = serde_json::to_vec(o).ok()?.len();
        if size > 32 * 1024 {
            return None;
        }
        bytes = bytes.checked_add(32 * 1024 - size)?;
    }
    Some(bytes)
}
fn objects(v: &Value) -> bool {
    v.is_object()
        && v["schedules"].as_array().is_some_and(|a| {
            a.iter().all(|s| {
                s.is_object()
                    && s["generations"]
                        .as_array()
                        .is_some_and(|a| a.iter().all(Value::is_object))
                    && s["occurrences"]
                        .as_array()
                        .is_some_and(|a| a.iter().all(Value::is_object))
                    && (s["last_skip"].is_null() || s["last_skip"].is_object())
            })
        })
}
fn parse(b: &[u8]) -> Result<Value, Fault> {
    crate::request::parse_bounded_json(b, 96)
        .map_err(|_| Fault::new("schedule_store_recovery_required"))
}
fn publish(
    root: &Directory,
    name: &str,
    v: &Value,
    replace: bool,
    c: &RunControl,
    d: Instant,
) -> Result<(), Fault> {
    let bytes = serde_json::to_vec(v).unwrap();
    if bytes.len() > MAX_STATE {
        return Err(Fault::new("schedule_quota_exceeded"));
    }
    active(c, d)?;
    let temporary = crate::result::new_id("pending");
    let mut f = root.create_file(&temporary).map_err(converted)?;
    (|| {
        for chunk in bytes.chunks(32768) {
            active(c, d)?;
            f.write_all(chunk)
                .map_err(|_| Fault::new("schedule_store_io"))?;
        }
        f.sync_all().map_err(|_| Fault::new("schedule_store_io"))?;
        active(c, d)?;
        #[cfg(test)]
        if FAIL_BEFORE_PUBLISH.with(|f| f.replace(false)) {
            return Err(Fault::new("schedule_test_write_failed"));
        }
        root.publish(&temporary, name, replace).map_err(converted)?;
        root.sync().map_err(converted)
    })()
    .map_err(Fault::after_effects)
}
#[cfg(test)]
thread_local! {pub(super) static FAIL_BEFORE_PUBLISH:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
