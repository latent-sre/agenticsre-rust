use super::*;
use crate::evidence::store::{
    StoreError,
    fs::{self, Directory},
};
use model::{Admission, State};
use std::{fs::File, io::Write, os::fd::AsRawFd, path::Path};
const MAX_JOBS: usize = 64;
const MAX_TOTAL: u64 = 128 * 1024 * 1024;
const MAX_ADMISSION: usize = 128 * 1024;
const MAX_STATE: usize = 64 * 1024;
fn converted(error: StoreError) -> Failure {
    Failure {
        code: match error.code {
            "store_busy" => "job_store_busy",
            "store_not_found" => "job_store_not_found",
            "store_no_space" => "job_store_no_space",
            "store_permission_denied" => "job_store_permission_denied",
            _ => "job_store_recovery_required",
        },
        effects: error.effects,
    }
}
pub(super) struct Store {
    root: Directory,
    _lock: File,
    pub location: Value,
    pub created: bool,
    deadline: Instant,
}
pub(super) struct Job {
    pub admission: Admission,
    pub state: State,
    pub directory: Directory,
}
impl Store {
    pub fn open(
        path: &Path,
        create: bool,
        control: &RunControl,
        deadline: Instant,
    ) -> Result<Self, Failure> {
        active(control, deadline)?;
        if !path.is_absolute()
            || path
                .to_str()
                .is_none_or(|s| s.len() > 4096 || s.chars().any(char::is_control))
        {
            return Err(Failure::new("invalid_job_store_path"));
        }
        let (root, created) = fs::open_root(path, create).map_err(converted)?;
        let opened=(||{
            let initial=root.names(MAX_JOBS+2).map_err(converted)?;
            let initialize=create&&initial.is_empty();
            if !initialize&&(!initial.iter().any(|n|n=="format.json")||!initial.iter().any(|n|n=="lock")){return Err(Failure::new("job_store_recovery_required"));}
            let (lock,lock_created)=loop{
                active(control,deadline)?;
                match fs::lock(&root,initialize){
                    Ok(lock)=>break lock,
                    Err(error)if error.code=="store_busy"=>std::thread::sleep(Duration::from_millis(10)),
                    Err(error)=>return Err(converted(error)),
                }
            };
            let setup=(||{
                if initialize{
                    if !lock_created||root.names(2).map_err(converted)?!=["lock"]{return Err(Failure::new("job_store_recovery_required"));}
                    let format=json!({"format_version":1,"kind":"local-jobs","store_id":crate::result::new_id("jobstore"),"owner_uid":uid()});
                    publish(&root,"format.json",&serde_json::to_vec(&format).expect("format"),false,deadline)?;
                }
                let format=crate::request::parse_bounded_json(&fs::read(&root,"format.json",1024).map_err(converted)?,4).map_err(|_|Failure::new("job_store_recovery_required"))?;
                if !keys(&format,&["format_version","kind","store_id","owner_uid"])||format["format_version"]!=1||format["kind"]!="local-jobs"||format["owner_uid"]!=uid()||format["store_id"].as_str().is_none_or(|s|s.is_empty()||s.len()>128){return Err(Failure::new("job_store_recovery_required"));}
                use std::os::unix::fs::MetadataExt;
                let metadata=root.0.metadata().map_err(|_|Failure::new("job_store_io"))?;
                let location=json!({"kind":"local","store_id":format["store_id"],"root":path,"device":metadata.dev(),"inode":metadata.ino()});
                Ok(location)
            })().map_err(|e:Failure|if created||lock_created{e.after_effects()}else{e})?;
            Ok((lock,setup,created||lock_created))
        })().map_err(|e:Failure|if created{e.after_effects()}else{e})?;
        let store = Self {
            root,
            _lock: opened.0,
            location: opened.1,
            created: opened.2,
            deadline,
        };
        store
            .scan()
            .map_err(|e| if store.created { e.after_effects() } else { e })?;
        Ok(store)
    }
    fn scan(&self) -> Result<(Vec<String>, u64), Failure> {
        check(self.deadline)?;
        let mut ids = vec![];
        let mut total = 0;
        for name in self.root.names(MAX_JOBS + 2).map_err(converted)? {
            if matches!(name.as_str(), "lock" | "format.json") {
                let file = self.root.open_file(&name, false).map_err(converted)?;
                total += file
                    .metadata()
                    .map_err(|_| Failure::new("job_store_io"))?
                    .len();
                continue;
            }
            if !model::valid_id(&name) {
                return Err(Failure::new("job_store_recovery_required"));
            }
            let directory = self.root.open_at(&name, true).map_err(converted)?;
            let names = directory.names(4).map_err(converted)?;
            if !["request.json", "state.json", "lease"]
                .iter()
                .all(|name| names.iter().any(|n| n == name))
            {
                return Err(Failure::new("job_store_recovery_required"));
            }
            for file_name in names {
                check(self.deadline)?;
                let max = match file_name.as_str() {
                    "request.json" => MAX_ADMISSION,
                    "state.json" => MAX_STATE,
                    "lease" => 0,
                    "result.json" => model::MAX_RESULT,
                    _ => return Err(Failure::new("job_store_recovery_required")),
                };
                let size = directory
                    .open_file(&file_name, false)
                    .map_err(converted)?
                    .metadata()
                    .map_err(|_| Failure::new("job_store_io"))?
                    .len();
                if size > max as u64 {
                    return Err(Failure::new("job_store_recovery_required"));
                }
                total += size;
            }
            ids.push(name);
        }
        if ids.len() > MAX_JOBS || total > MAX_TOTAL {
            return Err(Failure::new("job_quota_exceeded"));
        }
        Ok((ids, total))
    }
    pub fn ids(&self) -> Result<Vec<String>, Failure> {
        Ok(self.scan()?.0)
    }
    pub fn load(&self, id: &str) -> Result<Job, Failure> {
        if !model::valid_id(id) {
            return Err(Failure::new("invalid_job_input"));
        }
        let directory = self.root.open_at(id, true).map_err(converted)?;
        let raw = fs::read(&directory, "request.json", MAX_ADMISSION).map_err(converted)?;
        let value = crate::request::parse_bounded_json(&raw, 24)
            .map_err(|_| Failure::new("job_store_recovery_required"))?;
        if !value.is_object()
            || ![
                "request",
                "request/target",
                "request/limits",
                "request/inputs",
                "implementation",
            ]
            .iter()
            .all(|path| {
                value
                    .pointer(&format!("/{path}"))
                    .is_some_and(Value::is_object)
            })
        {
            return Err(Failure::new("job_store_recovery_required"));
        }
        let admission: Admission = serde_json::from_value(value)
            .map_err(|_| Failure::new("job_store_recovery_required"))?;
        let value = crate::request::parse_bounded_json(
            &fs::read(&directory, "state.json", MAX_STATE).map_err(converted)?,
            8,
        )
        .map_err(|_| Failure::new("job_store_recovery_required"))?;
        if !value.is_object()
            || value["events"]
                .as_array()
                .is_none_or(|events| events.iter().any(|e| !e.is_object()))
        {
            return Err(Failure::new("job_store_recovery_required"));
        }
        let state: State = serde_json::from_value(value)
            .map_err(|_| Failure::new("job_store_recovery_required"))?;
        if !admission.valid(id) || !state.valid(&admission) {
            return Err(Failure::new("job_store_recovery_required"));
        }
        check(self.deadline)?;
        Ok(Job {
            admission,
            state,
            directory,
        })
    }
    pub fn create(&self, admission: Admission) -> Result<Job, Failure> {
        let (ids, total) = self.scan()?;
        let state = State::new(&admission);
        let request = serde_json::to_vec(&admission).expect("admission");
        let state_bytes = serde_json::to_vec(&state).expect("state");
        if ids.len() >= MAX_JOBS
            || request.len() > MAX_ADMISSION
            || state_bytes.len() > MAX_STATE
            || total + (request.len() + state_bytes.len()) as u64 > MAX_TOTAL
        {
            return Err(Failure::new("job_quota_exceeded"));
        }
        check(self.deadline)?;
        let directory = self.root.create_dir(&admission.job_id).map_err(converted)?;
        let initialized = (|| {
            let lease = directory.create_file("lease").map_err(converted)?;
            lease.sync_all().map_err(|_| Failure::new("job_store_io"))?;
            publish(&directory, "request.json", &request, false, self.deadline)?;
            publish(&directory, "state.json", &state_bytes, false, self.deadline)?;
            self.root.sync().map_err(converted)?;
            Ok(())
        })()
        .map_err(Failure::after_effects);
        initialized?;
        Ok(Job {
            admission,
            state,
            directory,
        })
    }
    pub fn commit(&self, job: &Job) -> Result<(), Failure> {
        if !job.state.valid(&job.admission) {
            return Err(Failure::new("job_invalid_transition"));
        }
        let bytes = serde_json::to_vec(&job.state).expect("state");
        self.reserve(bytes.len(), MAX_STATE)?;
        publish(&job.directory, "state.json", &bytes, true, self.deadline)
    }
    fn reserve(&self, bytes: usize, maximum: usize) -> Result<(), Failure> {
        if bytes > maximum || self.scan()?.1 + bytes as u64 > MAX_TOTAL {
            Err(Failure::new("job_quota_exceeded"))
        } else {
            Ok(())
        }
    }
    pub fn publish_result(&self, job: &Job, bytes: &[u8]) -> Result<(), Failure> {
        self.reserve(bytes.len(), model::MAX_RESULT)?;
        if job.state.result_sha256.as_deref() != Some(&model::digest(bytes)) {
            return Err(Failure::new("job_invalid_transition"));
        }
        publish(&job.directory, "result.json", bytes, false, self.deadline)
    }
    pub fn result(&self, job: &Job) -> Result<Option<Value>, Failure> {
        let names = job.directory.names(4).map_err(converted)?;
        if !names.iter().any(|name| name == "result.json") {
            return if job.state.state == "terminal" {
                Err(Failure::new("job_store_recovery_required"))
            } else {
                Ok(None)
            };
        }
        let bytes =
            fs::read(&job.directory, "result.json", model::MAX_RESULT).map_err(converted)?;
        if job.state.result_sha256.as_deref() != Some(&model::digest(&bytes)) {
            return Err(Failure::new("job_store_recovery_required"));
        }
        let result = crate::request::parse_bounded_json(&bytes, crate::result::MAX_RECEIPT_DEPTH)
            .map_err(|_| Failure::new("job_store_recovery_required"))?;
        if !result.is_object()
            || result["request_id"] != job.admission.request.request_id
            || result["run_id"] != job.admission.run_id
            || result["operation"] != job.admission.request.operation
            || result["operation_version"] != 1
            || result["spec_version"] != "0.1"
            || result["execution"]["status"]
                .as_str()
                .is_none_or(|s| !model::status(s))
        {
            return Err(Failure::new("job_store_recovery_required"));
        }
        if job.state.execution_status.as_deref() != result["execution"]["status"].as_str()
            || result["effect_outcome"].as_str() != Some(job.state.effect_outcome.as_str())
        {
            return Err(Failure::new("job_store_recovery_required"));
        }
        Ok(Some(result))
    }
}
pub(super) fn claim(job: &Job) -> Result<Option<File>, Failure> {
    let file = job.directory.open_file("lease", true).map_err(converted)?;
    // SAFETY: a live independently opened lease descriptor receives a nonblocking advisory lock.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(file));
    }
    if std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(None)
    } else {
        Err(Failure::new("job_store_io"))
    }
}
fn check(deadline: Instant) -> Result<(), Failure> {
    if Instant::now() >= deadline {
        Err(Failure::new("job_deadline_exceeded"))
    } else {
        Ok(())
    }
}
fn publish(
    directory: &Directory,
    name: &str,
    bytes: &[u8],
    replace: bool,
    deadline: Instant,
) -> Result<(), Failure> {
    check(deadline)?;
    let pending = crate::result::new_id("pending");
    let mut file = directory.create_file(&pending).map_err(converted)?;
    (|| {
        for chunk in bytes.chunks(64 * 1024) {
            check(deadline)?;
            file.write_all(chunk)
                .map_err(|_| Failure::new("job_store_io"))?;
        }
        file.sync_all().map_err(|_| Failure::new("job_store_io"))?;
        check(deadline)?;
        #[cfg(test)]
        if FAIL_BEFORE_PUBLISH.with(|flag| flag.replace(false)) {
            return Err(Failure::new("job_test_write_failed"));
        }
        directory
            .publish(&pending, name, replace)
            .map_err(converted)?;
        directory.sync().map_err(converted)?;
        Ok(())
    })()
    .map_err(Failure::after_effects)
}
#[cfg(test)]
thread_local! {pub(super) static FAIL_BEFORE_PUBLISH:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
