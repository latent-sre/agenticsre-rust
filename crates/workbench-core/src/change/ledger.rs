use super::fs::{Prepared, Snapshot};
use crate::evidence::store::fs::{self, Directory};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Write,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::Instant,
};

pub(super) const RECOVERY: &str = "change_recovery_required";
const MAX: usize = 64 * 1024 * 1024;
#[cfg(test)]
pub(super) static DISK_TEST_LEASE: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Approval {
    pub id: String,
    pub principal: String,
    pub certificate_sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub plan: Value,
    pub plan_sha256: String,
    pub approval: Option<Approval>,
    pub consumed: bool,
    pub prepared: Option<Prepared>,
    pub state: String,
    pub effect_outcome: String,
}
impl Record {
    pub fn original(&self) -> Result<Snapshot, &'static str> {
        serde_json::from_value(self.plan["original_identity"].clone()).map_err(|_| RECOVERY)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Disk {
    format_version: u64,
    owner: Value,
    records: BTreeMap<String, Record>,
}
pub(super) struct Ledger {
    pub directory: Directory,
    pub path: PathBuf,
    lease: File,
    disk: Disk,
    pub anchors: BTreeMap<String, Instant>,
    pub last_wall: u64,
    pub poisoned: bool,
}
fn storage<T>(result: Result<T, crate::evidence::store::StoreError>) -> Result<T, &'static str> {
    result.map_err(|_| RECOVERY)
}
impl Ledger {
    pub fn open(path: &Path, runner: &str) -> Result<Self, &'static str> {
        let (directory, _) = storage(fs::open_root(path, true))?;
        let names = storage(directory.names(4))?;
        let fresh = names.is_empty();
        let (lease, _) = fs::lock(&directory, fresh).map_err(|e| {
            if e.code == "store_busy" {
                "change_store_busy"
            } else {
                RECOVERY
            }
        })?;
        let metadata = directory.0.metadata().map_err(|_| RECOVERY)?;
        let owner = json!({"runner_id":runner,"device":metadata.dev(),"inode":metadata.ino(),"uid":metadata.uid()});
        if names.iter().any(|n| n != "lock" && n != "ledger.json") {
            return Err(RECOVERY);
        }
        let disk = if fresh {
            Disk {
                format_version: 1,
                owner,
                records: BTreeMap::new(),
            }
        } else {
            let bytes = storage(fs::read(&directory, "ledger.json", MAX))?;
            let disk: Disk = serde_json::from_slice(&bytes).map_err(|_| RECOVERY)?;
            if disk.format_version != 1 || disk.owner != owner || disk.records.len() > 32 {
                return Err(RECOVERY);
            }
            for (key, record) in &disk.records {
                let requester = record.plan["requester"].as_str().ok_or(RECOVERY)?;
                let request_id = record.plan["request_id"].as_str().ok_or(RECOVERY)?;
                if !super::client::valid_plan(&record.plan)
                    || *key
                        != super::hash(
                            &serde_json::to_vec(&(requester, request_id)).map_err(|_| RECOVERY)?,
                        )
                    || super::hash(&serde_json::to_vec(&record.plan).map_err(|_| RECOVERY)?)
                        != record.plan_sha256
                    || record.original().is_err()
                    || ![
                        "planned",
                        "approved",
                        "dispatching",
                        "verified",
                        "not_applied",
                        "unknown",
                    ]
                    .contains(&record.state.as_str())
                    || !["not_attempted", "verified", "unknown"]
                        .contains(&record.effect_outcome.as_str())
                    || record.consumed != record.prepared.is_some()
                    || (record.consumed && record.approval.is_none())
                    || (record.consumed && matches!(record.state.as_str(), "planned" | "approved"))
                    || (!record.consumed
                        && !matches!(record.state.as_str(), "planned" | "approved"))
                    || record
                        .prepared
                        .as_ref()
                        .is_some_and(|p| p.snapshot.sha256 != record.plan["proposed_sha256"])
                    || (record.state == "verified") != (record.effect_outcome == "verified")
                    || (record.approval.is_none() && record.state != "planned")
                    || record.approval.as_ref().is_some_and(|a| {
                        !super::pin(&a.id)
                            || !super::pin(&a.certificate_sha256)
                            || !crate::request::valid_operation(&a.principal)
                            || a.principal == requester
                    })
                {
                    return Err(RECOVERY);
                }
            }
            disk
        };
        let mut result = Self {
            directory,
            path: path.to_owned(),
            lease,
            disk,
            anchors: BTreeMap::new(),
            last_wall: 0,
            poisoned: false,
        };
        if fresh {
            result.save()?;
        }
        Ok(result)
    }
    pub fn owner(&self) -> Value {
        json!({"device":self.disk.owner["device"],"inode":self.disk.owner["inode"]})
    }
    pub fn records(&self) -> &BTreeMap<String, Record> {
        &self.disk.records
    }
    pub fn insert(&mut self, key: String, record: Record) {
        self.disk.records.insert(key, record);
    }
    pub fn check(&mut self) -> Result<(), &'static str> {
        if self.poisoned {
            return Err(RECOVERY);
        }
        let check = (|| {
            let (current, _) = storage(fs::open_root(&self.path, false))?;
            let metadata = current.0.metadata().map_err(|_| RECOVERY)?;
            let named_lock = storage(current.open_file("lock", false))?
                .metadata()
                .map_err(|_| RECOVERY)?;
            let held_lock = self.lease.metadata().map_err(|_| RECOVERY)?;
            if self.disk.owner["device"] != metadata.dev()
                || self.disk.owner["inode"] != metadata.ino()
                || named_lock.dev() != held_lock.dev()
                || named_lock.ino() != held_lock.ino()
                || held_lock.nlink() != 1
                || held_lock.len() != 0
                || storage(current.names(4))? != vec!["ledger.json".to_owned(), "lock".to_owned()]
                || storage(fs::read(&current, "ledger.json", MAX))?
                    != serde_json::to_vec(&self.disk).map_err(|_| RECOVERY)?
            {
                return Err(RECOVERY);
            }
            Ok(())
        })();
        if check.is_err() {
            self.poisoned = true;
        }
        check
    }
    pub fn save(&mut self) -> Result<(), &'static str> {
        let result = (|| {
            let bytes = serde_json::to_vec(&self.disk).map_err(|_| RECOVERY)?;
            if bytes.len() > MAX {
                return Err("change_store_full");
            }
            let name = crate::result::new_id("ledger");
            let mut file = storage(self.directory.create_file(&name))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| RECOVERY)?;
            storage(self.directory.publish(&name, "ledger.json", true))?;
            storage(self.directory.sync())
        })();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacing_the_lifetime_lease_poisons_the_original_service() {
        let root = std::env::temp_dir().join(crate::result::new_id("change-lease"));
        let mut ledger = Ledger::open(&root, "runner").unwrap();
        std::fs::remove_file(root.join("lock")).unwrap();
        ledger.directory.create_file("lock").unwrap();
        assert_eq!(ledger.check().unwrap_err(), RECOVERY);
        assert!(ledger.poisoned);
        drop(ledger);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires tools/test-sandbox.sh --small-evidence-disk"]
    fn full_private_disk_keeps_old_ledger_and_requires_recovery() {
        let _disk_guard = DISK_TEST_LEASE.lock().unwrap();
        let mount = Path::new("/tmp/evidence-small-disk");
        assert!(
            mount.is_dir(),
            "Use the declared small private disk sandbox"
        );
        let root = mount.join(crate::result::new_id("change-ledger"));
        let mut ledger = Ledger::open(&root, "runner").unwrap();
        let prior = std::fs::read(root.join("ledger.json")).unwrap();
        let fill = mount.join(crate::result::new_id("fill"));
        let mut filler = std::fs::File::create(&fill).unwrap();
        let mut exhausted = false;
        for _ in 0..64 {
            if filler.write_all(&[0; 4096]).is_err() {
                exhausted = true;
                break;
            }
        }
        assert!(exhausted, "The bounded 64KiB test mount must report full");
        assert_eq!(ledger.save().unwrap_err(), RECOVERY);
        assert!(ledger.poisoned);
        assert_eq!(std::fs::read(root.join("ledger.json")).unwrap(), prior);
        drop(ledger);
        assert_eq!(Ledger::open(&root, "runner").err().unwrap(), RECOVERY);
        std::fs::remove_file(fill).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
