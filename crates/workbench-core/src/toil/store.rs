use super::*;
use crate::evidence::store::{
    StoreError,
    fs::{self, Directory},
};
use std::{
    collections::BTreeSet, fs::File, io::Write, path::Path, sync::atomic::Ordering, time::Instant,
};
pub(super) const MAX_SAMPLES: usize = 1024;
const MAX_BYTES: usize = 4 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    pub format_version: u64,
    pub enabled: bool,
    pub retention_days: u64,
    pub updated_at: String,
    pub samples: Vec<Sample>,
}
pub(super) struct Store {
    directory: Directory,
    _lock: File,
    pub state: State,
    pub initialized: bool,
    pub created_effects: bool,
}
pub(super) fn error(code: &'static str) -> StoreError {
    StoreError::new(code, false)
}
pub(super) fn active(control: &RunControl, deadline: Instant) -> Result<(), StoreError> {
    if control.signal.load(Ordering::Relaxed) != 0 {
        return Err(error("toil_cancelled"));
    }
    if Instant::now() >= deadline {
        return Err(error("toil_deadline_exceeded"));
    }
    if let Some(fd) = control.result_fd {
        let mut p = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: p is initialized and borrowed for a nonblocking poll.
        if unsafe { libc::poll(&mut p, 1, 0) } > 0
            && p.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
        {
            return Err(error("toil_result_consumer_closed"));
        }
    }
    Ok(())
}
impl Store {
    pub fn open(
        path: &Path,
        create: bool,
        control: &RunControl,
        deadline: Instant,
    ) -> Result<Self, StoreError> {
        active(control, deadline)?;
        let (directory, created) = fs::open_root(path, create)?;
        let opened = (|| {
            let names = directory.names(3)?;
            if names
                .iter()
                .any(|name| !matches!(name.as_str(), "lock" | "state.json"))
            {
                return Err(error("toil_unknown_layout"));
            }
            let initialized = names.iter().any(|name| name == "state.json");
            if initialized && !names.iter().any(|name| name == "lock") {
                return Err(error("toil_store_corrupt"));
            }
            if !initialized && (!create || !names.is_empty()) {
                return Err(error("toil_unconfigured"));
            }
            let (lock, lock_created) = fs::lock(&directory, create && !initialized)?;
            let loaded = (|| {
                active(control, deadline)?;
                let names = directory.names(3)?;
                if names
                    .iter()
                    .any(|name| !matches!(name.as_str(), "lock" | "state.json"))
                {
                    return Err(error("toil_unknown_layout"));
                }
                let initialized = names.iter().any(|name| name == "state.json");
                if !initialized && !lock_created {
                    return Err(error("toil_unconfigured"));
                }
                let state = if initialized {
                    let bytes = fs::read(&directory, "state.json", MAX_BYTES)?;
                    let value = crate::request::parse_bounded_json(&bytes, 12)
                        .map_err(|_| error("toil_store_corrupt"))?;
                    if value["format_version"] != 1 {
                        return Err(error("toil_unsupported_format"));
                    }
                    if value["samples"]
                        .as_array()
                        .is_none_or(|samples| samples.iter().any(|sample| !sample.is_object()))
                    {
                        return Err(error("toil_store_corrupt"));
                    }
                    let state: State =
                        serde_json::from_slice(&bytes).map_err(|_| error("toil_store_corrupt"))?;
                    let mut ids = BTreeSet::new();
                    if !(1..=3650).contains(&state.retention_days)
                        || utc(&json!(state.updated_at)).is_none()
                        || state.samples.len() > MAX_SAMPLES
                    {
                        return Err(error("toil_store_corrupt"));
                    }
                    for sample in &state.samples {
                        active(control, deadline)?;
                        if !sample.valid() || !ids.insert(&sample.id) {
                            return Err(error("toil_store_corrupt"));
                        }
                    }
                    state
                } else {
                    State {
                        format_version: 1,
                        enabled: false,
                        retention_days: 7,
                        updated_at: crate::result::timestamp(),
                        samples: vec![],
                    }
                };
                Ok((state, initialized))
            })()
            .map_err(|e: StoreError| {
                if created || lock_created {
                    e.after_effects()
                } else {
                    e
                }
            })?;
            Ok(Self {
                directory,
                _lock: lock,
                state: loaded.0,
                initialized: loaded.1,
                created_effects: created || lock_created,
            })
        })();
        opened.map_err(|e: StoreError| if created { e.after_effects() } else { e })
    }
    pub fn ensure_consent(&self) -> Result<(), StoreError> {
        if self.state.enabled {
            Ok(())
        } else {
            Err(error("toil_disabled"))
        }
    }
    pub fn ensure_capacity(&self) -> Result<(), StoreError> {
        if self.state.samples.len() >= MAX_SAMPLES
            || serde_json::to_vec(&self.state).expect("state").len() + 2048 > MAX_BYTES
        {
            Err(error("toil_quota_exceeded"))
        } else {
            Ok(())
        }
    }
    pub fn commit(&mut self, control: &RunControl, deadline: Instant) -> Result<(), StoreError> {
        active(control, deadline)?;
        self.state.updated_at = crate::result::timestamp();
        let bytes = serde_json::to_vec(&self.state).expect("state");
        if bytes.len() > MAX_BYTES || self.state.samples.len() > MAX_SAMPLES {
            return Err(error("toil_quota_exceeded"));
        }
        let temporary = crate::result::new_id("pending");
        let mut file = self.directory.create_file(&temporary)?;
        let mut published = false;
        let write = (|| {
            for chunk in bytes.chunks(16 * 1024) {
                active(control, deadline)?;
                file.write_all(chunk).map_err(|e| fs::io_error(e, true))?;
            }
            file.sync_all().map_err(|e| fs::io_error(e, true))?;
            active(control, deadline)?;
            #[cfg(test)]
            if FAIL_BEFORE_PUBLISH.with(|flag| flag.replace(false)) {
                return Err(error("toil_test_write_failed"));
            }
            self.directory
                .publish(&temporary, "state.json", self.initialized)?;
            published = true;
            self.directory.sync()?;
            self.initialized = true;
            Ok(())
        })();
        if write.is_err() && !published {
            let cleanup = self
                .directory
                .unlink(&temporary, false)
                .and_then(|_| self.directory.sync());
            if cleanup.is_err() {
                return Err(error("toil_incomplete_write").after_effects());
            }
        }
        write.map_err(|e: StoreError| if published { e.after_effects() } else { e })
    }
}
#[cfg(test)]
thread_local! {pub(super) static FAIL_BEFORE_PUBLISH:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
