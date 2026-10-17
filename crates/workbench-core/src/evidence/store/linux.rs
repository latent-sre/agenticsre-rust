use super::{
    ArtifactRef, BundleRef, BundleValidator, EvidenceStoreOptions, MAX_ARTIFACTS,
    MAX_COLLECTION_BYTES, MAX_METADATA_BYTES, MAX_RUNS, ManagementProgress, Manifest, RunSummary,
    StoreError, VerifiedBundle, lower_hex, valid_run_id,
};
use crate::request::{MAX_RESULT_BYTES, parse_bounded_json};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use super::fs::{self, Directory, io_error};

const MAX_PAYLOAD_BYTES: usize = 128 * 1024;
const MAX_STAGING_FILES: usize = 32;
const MAX_ROOT_ENTRIES: usize = MAX_RUNS + MAX_STAGING_FILES + 3;
const FORMAT_VERSION: u64 = 1;
const RUN_METADATA: [&str; 5] = [
    "intent.json",
    "checkpoint.json",
    "manifest.json",
    "protection.json",
    "deletion.json",
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Format {
    format_version: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    format_version: u64,
    id: String,
    created_at_unix_ms: u64,
    reservation: u64,
    payload: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    format_version: u64,
    id: String,
    artifacts: BTreeMap<String, ArtifactRef>,
    payload: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Protection {
    format_version: u64,
    id: String,
    protected: bool,
}

#[derive(Default)]
struct RunLayout {
    names: Vec<String>,
    artifact_names: Vec<String>,
    bytes: u64,
    version_error: Option<&'static str>,
}

impl RunLayout {
    fn has(&self, name: &str) -> bool {
        self.names.iter().any(|entry| entry == name)
    }

    fn unresolved(&self) -> bool {
        self.names.iter().any(|name| temporary_name(name))
            || self.artifact_names.iter().any(|name| temporary_name(name))
    }
}

/// The one process-wide store lock is retained until this value and any lease are dropped.
pub(crate) struct Store {
    root: Directory,
    runs: Directory,
    _lock: File,
    options: EvidenceStoreOptions,
    deadline: Instant,
    used_bytes: u64,
    mutation_error: Option<&'static str>,
    poisoned: bool,
    validator: Option<BundleValidator>,
    management_signal: Option<Arc<AtomicUsize>>,
    management_effects: bool,
    progress: ManagementProgress,
}

pub(crate) struct CollectionLease<'a> {
    store: &'a mut Store,
    run: Directory,
    objects: Directory,
    id: String,
    created_at_unix_ms: u64,
    initial_bytes: u64,
    reservation: u64,
    artifacts: BTreeMap<String, ArtifactRef>,
    payload: Value,
    sealed: bool,
}

fn check_deadline(deadline: Instant) -> Result<(), StoreError> {
    if Instant::now() >= deadline {
        Err(StoreError::new("store_deadline_exceeded", false))
    } else {
        Ok(())
    }
}

fn check_control(deadline: Instant, signal: Option<&AtomicUsize>) -> Result<(), StoreError> {
    if signal.is_some_and(|signal| signal.load(Ordering::Relaxed) != 0) {
        return Err(StoreError::new("store_cancelled", false));
    }
    check_deadline(deadline)
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(output, "{byte:02x}").expect("string formatting");
    }
    output
}

fn temporary() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    format!(
        ".tmp-{:x}-{:x}-{:x}",
        std::process::id(),
        now_unix_ms(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn temporary_name(name: &str) -> bool {
    if name.len() > 96 {
        return false;
    }
    let Some(suffix) = name.strip_prefix(".tmp-") else {
        return false;
    };
    let parts: Vec<_> = suffix.split('-').collect();
    parts.len() == 3 && parts.iter().all(|part| lower_hex(part))
}

fn artifact_name(name: &str) -> bool {
    name.strip_suffix(".json")
        .is_some_and(|value| value.len() == 64 && lower_hex(value))
}

fn journal_name(id: &str) -> String {
    format!(".delete-{id}.json")
}

fn journal_id(name: &str) -> Option<&str> {
    name.strip_prefix(".delete-")?
        .strip_suffix(".json")
        .filter(|id| valid_run_id(id))
}

fn step_id(id: &str) -> bool {
    (1..=80).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn json_bytes(value: &impl Serialize) -> Result<Vec<u8>, StoreError> {
    let mut bytes =
        serde_json::to_vec(value).map_err(|_| StoreError::new("store_invalid_metadata", false))?;
    bytes.push(b'\n');
    if bytes.len() > MAX_METADATA_BYTES {
        return Err(StoreError::new("store_object_too_large", false));
    }
    Ok(bytes)
}

fn payload(value: &Value) -> Result<(), StoreError> {
    if !value.is_object() {
        return Err(StoreError::new("store_invalid_metadata", false));
    }
    let bytes =
        serde_json::to_vec(value).map_err(|_| StoreError::new("store_invalid_metadata", false))?;
    if bytes.len() > MAX_PAYLOAD_BYTES || parse_bounded_json(&bytes, 32).is_err() {
        return Err(StoreError::new("store_invalid_metadata", false));
    }
    Ok(())
}

fn version(bytes: &[u8]) -> Result<Value, StoreError> {
    let value =
        parse_bounded_json(bytes, 40).map_err(|_| StoreError::new("store_corrupt", false))?;
    match value.get("format_version").and_then(Value::as_u64) {
        Some(FORMAT_VERSION) if value.is_object() => Ok(value),
        Some(_) => Err(StoreError::new("store_unsupported_format", false)),
        None => Err(StoreError::new("store_corrupt", false)),
    }
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, StoreError> {
    serde_json::from_value(version(bytes)?).map_err(|_| StoreError::new("store_corrupt", false))
}

fn read_json<T: DeserializeOwned>(directory: &Directory, name: &str) -> Result<T, StoreError> {
    decode(&fs::read(directory, name, MAX_METADATA_BYTES)?)
}

fn validate_refs(artifacts: &BTreeMap<String, ArtifactRef>) -> Result<(), StoreError> {
    let mut total = 0_u64;
    if artifacts.len() > MAX_ARTIFACTS {
        return Err(StoreError::new("store_corrupt", false));
    }
    for (id, reference) in artifacts {
        let Some(hash) = reference.digest.strip_prefix("sha256:") else {
            return Err(StoreError::new("store_corrupt", false));
        };
        if !step_id(id)
            || hash.len() != 64
            || !lower_hex(hash)
            || reference.path != format!("artifacts/{hash}.json")
            || reference.media_type != "application/json"
            || reference.classification != "restricted"
            || !(1..=MAX_RESULT_BYTES as u64).contains(&reference.bytes)
        {
            return Err(StoreError::new("store_corrupt", false));
        }
        total = total.saturating_add(reference.bytes);
    }
    if total > MAX_COLLECTION_BYTES {
        return Err(StoreError::new("store_corrupt", false));
    }
    Ok(())
}

fn validate_receipt(bytes: &[u8]) -> Result<(), StoreError> {
    if bytes.is_empty()
        || bytes.len() > MAX_RESULT_BYTES
        || !bytes.ends_with(b"\n")
        || bytes.ends_with(b"\n\n")
    {
        return Err(StoreError::new("store_invalid_receipt", false));
    }
    let value = parse_bounded_json(bytes, crate::result::MAX_RECEIPT_DEPTH)
        .map_err(|_| StoreError::new("store_invalid_receipt", false))?;
    const KEYS: [&str; 21] = [
        "spec_version",
        "request_id",
        "run_id",
        "operation",
        "operation_version",
        "target",
        "resolved_target",
        "started_at",
        "finished_at",
        "duration_ms",
        "execution",
        "effect_outcome",
        "assessment",
        "coverage",
        "output",
        "data",
        "errors",
        "sources",
        "artifacts",
        "effective_limits",
        "record_mode",
    ];
    let object = value
        .as_object()
        .ok_or(StoreError::new("store_invalid_receipt", false))?;
    if value["spec_version"] != "0.1" || value["operation_version"] != 1 {
        return Err(StoreError::new("store_unsupported_receipt", false));
    }
    if object.len() != KEYS.len()
        || KEYS.iter().any(|key| !object.contains_key(*key))
        || value["operation"] != "query.run"
        || value["record_mode"] != "never"
        || value["assessment"] != "not_assessed"
        || !value["run_id"].as_str().is_some_and(valid_run_id)
        || !value["request_id"]
            .as_str()
            .is_some_and(|id| (1..=128).contains(&id.len()))
        || !value["duration_ms"].is_u64()
        || !value["effect_outcome"].is_string()
        || serde_json::from_value::<crate::request::Target>(value["target"].clone()).is_err()
        || serde_json::from_value::<crate::request::Limits>(value["effective_limits"].clone())
            .is_err()
        || ["resolved_target", "execution", "coverage", "output", "data"]
            .iter()
            .any(|key| !value[*key].is_object())
        || ["errors", "sources", "artifacts"]
            .iter()
            .any(|key| !value[*key].is_array())
        || ["started_at", "finished_at"].iter().any(|key| {
            value[*key].as_str().is_none_or(|timestamp| {
                time::OffsetDateTime::parse(
                    timestamp,
                    &time::format_description::well_known::Rfc3339,
                )
                .is_err()
            })
        })
        || !matches!(
            value["execution"]["status"].as_str(),
            Some(
                "succeeded"
                    | "failed"
                    | "partial"
                    | "denied"
                    | "unsupported"
                    | "cancelled"
                    | "timed_out"
                    | "unknown"
            )
        )
        || !value["coverage"]["state"].is_string()
        || !value["coverage"]["scope"].is_string()
        || !value["coverage"]["limitations"]
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string))
        || !value["output"]["stdout"].is_string()
        || !value["output"]["stderr"].is_string()
        || !value["output"]["stdout_truncated"].is_boolean()
        || !value["output"]["stderr_truncated"].is_boolean()
        || !strict_receipt_parts(&value)
    {
        return Err(StoreError::new("store_invalid_receipt", false));
    }
    Ok(())
}

fn object_keys(value: &Value, required: &[&str], optional: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        required.iter().all(|key| object.contains_key(*key))
            && object
                .keys()
                .all(|key| required.contains(&key.as_str()) || optional.contains(&key.as_str()))
    })
}

fn text_length(value: &Value, min: usize, max: usize) -> bool {
    value
        .as_str()
        .is_some_and(|text| (min..=max).contains(&text.chars().count()))
}

fn timestamp(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).is_ok()
    })
}

fn strict_receipt_parts(value: &Value) -> bool {
    let execution = &value["execution"];
    let coverage = &value["coverage"];
    let output = &value["output"];
    let limits = &value["effective_limits"];
    let target = &value["target"];
    object_keys(execution, &["status", "child_exit_code", "signal"], &[])
        && (execution["child_exit_code"].is_null()
            || execution["child_exit_code"]
                .as_i64()
                .is_some_and(|code| i32::try_from(code).is_ok()))
        && (execution["signal"].is_null() || text_length(&execution["signal"], 0, 32))
        && matches!(
            value["effect_outcome"].as_str(),
            Some("not_attempted" | "not_applicable" | "succeeded" | "failed" | "unknown")
        )
        && (!matches!(execution["status"].as_str(), Some("denied" | "unsupported"))
            || value["effect_outcome"] == "not_attempted")
        && object_keys(coverage, &["state", "scope", "limitations"], &[])
        && matches!(
            coverage["state"].as_str(),
            Some("complete" | "partial" | "not_established" | "not_applicable")
        )
        && text_length(&coverage["scope"], 1, 2048)
        && coverage["limitations"].as_array().is_some_and(|items| {
            items.len() <= 50 && items.iter().all(|item| text_length(item, 1, 2048))
        })
        && object_keys(
            output,
            &[
                "stdout",
                "stderr",
                "stdout_truncated",
                "stderr_truncated",
                "encoding",
            ],
            &[],
        )
        && matches!(
            output["encoding"].as_str(),
            Some("utf-8" | "utf-8-replaced")
        )
        && object_keys(
            limits,
            &["timeout_ms", "max_output_bytes"],
            &["max_items", "concurrency"],
        )
        && limits["timeout_ms"]
            .as_u64()
            .is_some_and(|n| (1..=300_000).contains(&n))
        && limits["max_output_bytes"]
            .as_u64()
            .is_some_and(|n| (1024..=MAX_RESULT_BYTES as u64).contains(&n))
        && limits
            .get("max_items")
            .is_none_or(|value| value.as_u64().is_some_and(|n| (1..=1000).contains(&n)))
        && limits
            .get("concurrency")
            .is_none_or(|value| value.as_u64().is_some_and(|n| (1..=16).contains(&n)))
        && object_keys(target, &["kind", "id"], &[])
        && target["kind"] == "connection"
        && target["id"]
            .as_str()
            .is_some_and(crate::request::valid_operation)
        && value["errors"].as_array().is_some_and(|errors| {
            errors.len() <= 50
                && (!matches!(
                    execution["status"].as_str(),
                    Some("failed" | "denied" | "unsupported" | "timed_out" | "unknown")
                ) || !errors.is_empty())
                && errors.iter().all(|error| {
                    object_keys(error, &["code", "message", "retryable"], &[])
                        && error["code"].as_str().is_some_and(|code| {
                            (1..=128).contains(&code.len())
                                && code.as_bytes()[0].is_ascii_lowercase()
                                && code.bytes().all(|byte| {
                                    byte.is_ascii_lowercase()
                                        || byte.is_ascii_digit()
                                        || byte == b'_'
                                })
                        })
                        && text_length(&error["message"], 1, 2048)
                        && error["retryable"].is_boolean()
                })
        })
        && value["sources"].as_array().is_some_and(|sources| {
            sources.len() <= 100
                && sources.iter().all(|source| {
                    object_keys(
                        source,
                        &["reference", "observed_at", "evidence_state", "untrusted"],
                        &[],
                    ) && text_length(&source["reference"], 1, 2048)
                        && (source["observed_at"].is_null() || timestamp(&source["observed_at"]))
                        && matches!(
                            source["evidence_state"].as_str(),
                            Some("verified" | "sourced" | "unverified")
                        )
                        && source["untrusted"].is_boolean()
                })
        })
        && value["artifacts"].as_array().is_some_and(Vec::is_empty)
}

fn artifact_ref(bytes: &[u8]) -> ArtifactRef {
    let hash = sha256(bytes);
    ArtifactRef {
        path: format!("artifacts/{hash}.json"),
        media_type: "application/json".into(),
        bytes: bytes.len() as u64,
        digest: format!("sha256:{hash}"),
        classification: "restricted".into(),
    }
}

fn verify_artifact(objects: &Directory, reference: &ArtifactRef) -> Result<Vec<u8>, StoreError> {
    let filename = reference
        .path
        .strip_prefix("artifacts/")
        .filter(|name| artifact_name(name))
        .ok_or(StoreError::new("store_corrupt", false))?;
    let bytes = fs::read(objects, filename, MAX_RESULT_BYTES)?;
    if bytes.len() as u64 != reference.bytes
        || format!("sha256:{}", sha256(&bytes)) != reference.digest
    {
        return Err(StoreError::new("store_corrupt", false));
    }
    validate_receipt(&bytes)?;
    Ok(bytes)
}

fn initial_write(directory: &Directory, bytes: &[u8], deadline: Instant) -> Result<(), StoreError> {
    check_deadline(deadline)?;
    let temporary = temporary();
    let mut file = directory.create_file(&temporary)?;
    file.write_all(bytes).map_err(|e| io_error(e, true))?;
    check_deadline(deadline).map_err(StoreError::after_effects)?;
    file.sync_all().map_err(|e| io_error(e, true))?;
    check_deadline(deadline).map_err(StoreError::after_effects)?;
    directory.publish(&temporary, "format.json", false)?;
    directory.sync()
}

impl Store {
    #[cfg(test)]
    pub(crate) fn open(
        options: &EvidenceStoreOptions,
        create: bool,
        deadline: Instant,
    ) -> Result<Self, StoreError> {
        Self::open_inner(options, create, deadline, None, None)
    }

    pub(crate) fn open_validated(
        options: &EvidenceStoreOptions,
        create: bool,
        deadline: Instant,
        validator: BundleValidator,
        management_signal: Option<Arc<AtomicUsize>>,
    ) -> Result<Self, StoreError> {
        Self::open_inner(
            options,
            create,
            deadline,
            Some(validator),
            management_signal,
        )
    }

    fn open_inner(
        options: &EvidenceStoreOptions,
        create: bool,
        deadline: Instant,
        validator: Option<BundleValidator>,
        management_signal: Option<Arc<AtomicUsize>>,
    ) -> Result<Self, StoreError> {
        options.validate()?;
        check_control(deadline, management_signal.as_deref())?;
        let format = json_bytes(&Format {
            format_version: FORMAT_VERSION,
        })?;
        if create && options.max_bytes < format.len() as u64 {
            return Err(StoreError::new("store_quota_exceeded", false));
        }
        let (root, created) = fs::open_root(&options.directory, create)?;
        let mut effects = created;
        let initialized = (|| {
            // Inspect existing format before creating anything. A missing lock in an initialized
            // store is not recreated: another process may still hold its unlinked old inode.
            let existing = root.names(MAX_ROOT_ENTRIES)?;
            let has_format = existing.iter().any(|name| name == "format.json");
            for name in &existing {
                check_control(deadline, management_signal.as_deref())?;
                if name == "runs" {
                    root.open_at(name, true)?;
                } else if matches!(name.as_str(), "lock" | "format.json")
                    || temporary_name(name)
                    || journal_id(name).is_some()
                {
                    fs::read(&root, name, MAX_METADATA_BYTES)?;
                } else {
                    return Err(StoreError::new("store_unsafe_entry", false));
                }
            }
            if has_format {
                let _: Format = read_json(&root, "format.json")?;
                if !existing.iter().any(|name| name == "lock") {
                    return Err(StoreError::new("store_uninitialized", false));
                }
            } else if !create || existing.iter().any(|name| name != "lock") {
                return Err(StoreError::new("store_uninitialized", false));
            }
            let (lock, lock_created) = fs::lock(&root, create && !has_format)?;
            effects |= lock_created;
            let names = root.names(MAX_ROOT_ENTRIES)?;
            if !names.iter().any(|name| name == "format.json") {
                if !create || names.iter().any(|name| name != "lock") {
                    return Err(StoreError::new("store_uninitialized", lock_created));
                }
                initial_write(&root, &format, deadline)?;
                effects = true;
            } else {
                let _: Format = read_json(&root, "format.json")?;
            }
            let runs = match root.open_at("runs", true) {
                Ok(runs) => runs,
                Err(error) if create && error.code == "store_not_found" => {
                    let runs = root.create_dir("runs")?;
                    effects = true;
                    root.sync()?;
                    runs
                }
                Err(error) => return Err(error),
            };
            Ok((lock, runs))
        })()
        .map_err(|error| {
            if effects {
                error.after_effects()
            } else {
                error
            }
        })?;
        let management_effects = effects && management_signal.is_some();
        let mut store = Self {
            root,
            runs: initialized.1,
            _lock: initialized.0,
            options: options.clone(),
            deadline,
            used_bytes: 0,
            mutation_error: None,
            poisoned: false,
            validator,
            management_signal,
            management_effects,
            progress: ManagementProgress::default(),
        };
        store.rescan().map_err(|error| {
            if effects {
                error.after_effects()
            } else {
                error
            }
        })?;
        Ok(store)
    }

    pub(crate) fn progress(&self) -> &ManagementProgress {
        &self.progress
    }

    #[cfg(test)]
    fn cancel_after_progress(&self) {
        CANCEL_AFTER_PROGRESS.with(|limit| {
            if limit.get().is_some_and(|limit| {
                self.progress.deleted.len() + self.progress.recovered.len() >= limit
            }) {
                limit.set(None);
                if let Some(signal) = &self.management_signal {
                    signal.store(2, Ordering::Relaxed);
                }
            }
        });
    }

    fn management_error(&self, error: StoreError) -> StoreError {
        if self.management_effects {
            error.after_effects()
        } else {
            error
        }
    }

    fn check(&self) -> Result<(), StoreError> {
        check_control(self.deadline, self.management_signal.as_deref())
            .map_err(|error| self.management_error(error))
    }

    fn validate_bundle(&self, bundle: &VerifiedBundle) -> Result<(), StoreError> {
        self.check()?;
        if let Some(validate) = self.validator {
            validate(bundle)?;
        }
        self.check()
    }

    fn rescan(&mut self) -> Result<Vec<String>, StoreError> {
        self.check()?;
        let mut total = 0_u64;
        self.mutation_error = None;
        for name in self.root.names(MAX_ROOT_ENTRIES)? {
            self.check()?;
            if name == "runs" {
                self.root.open_at(&name, true)?;
            } else if matches!(name.as_str(), "lock" | "format.json")
                || temporary_name(&name)
                || journal_id(&name).is_some()
            {
                let file = self.root.open_file(&name, false)?;
                let length = file.metadata().map_err(|e| io_error(e, false))?.len();
                if length > MAX_METADATA_BYTES as u64 || (name == "lock" && length != 0) {
                    return Err(StoreError::new("store_object_too_large", false));
                }
                total = total.saturating_add(length);
                if journal_id(&name).is_some()
                    && let Err(error) = version(&fs::read(&self.root, &name, MAX_METADATA_BYTES)?)
                    && error.code == "store_unsupported_format"
                {
                    self.mutation_error = Some(error.code);
                }
            } else {
                return Err(StoreError::new("store_unsafe_entry", false));
            }
        }
        let ids = self.identifiers()?;
        for id in &ids {
            let layout = self.layout(id)?;
            total = total.saturating_add(layout.bytes);
            if layout.version_error == Some("store_unsupported_format") {
                self.mutation_error = layout.version_error;
            }
        }
        self.used_bytes = total;
        Ok(ids)
    }

    fn identifiers(&self) -> Result<Vec<String>, StoreError> {
        self.check()?;
        let mut ids: BTreeSet<String> = self.runs.names(MAX_RUNS)?.into_iter().collect();
        for name in self.root.names(MAX_ROOT_ENTRIES)? {
            self.check()?;
            if let Some(id) = journal_id(&name) {
                ids.insert(id.into());
            }
        }
        if ids.len() > MAX_RUNS {
            return Err(StoreError::new("store_layout_limit", false));
        }
        Ok(ids.into_iter().collect())
    }

    fn root_journal(&self, id: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.check()?;
        match fs::read(&self.root, &journal_name(id), MAX_METADATA_BYTES) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.code == "store_not_found" => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn layout(&self, id: &str) -> Result<RunLayout, StoreError> {
        self.check()?;
        if !valid_run_id(id) {
            return Err(StoreError::new("store_invalid_id", false));
        }
        let journal = self.root_journal(id)?;
        let journal_error = journal
            .as_deref()
            .and_then(|bytes| version(bytes).err())
            .map(|error| error.code);
        let run = match self.runs.open_at(id, true) {
            Ok(run) => run,
            Err(error) if error.code == "store_not_found" && journal.is_some() => {
                return Ok(RunLayout {
                    names: vec!["deletion.json".into()],
                    version_error: journal_error,
                    ..RunLayout::default()
                });
            }
            Err(error) => return Err(error),
        };
        let mut layout = RunLayout {
            names: run.names(MAX_STAGING_FILES + 6)?,
            ..RunLayout::default()
        };
        layout.version_error = journal_error;
        if journal.is_some() && (layout.has("deletion.json") || layout.has("manifest.json")) {
            return Err(StoreError::new("store_corrupt", false));
        }
        for name in &layout.names {
            self.check()?;
            if name == "artifacts" {
                let objects = run.open_at(name, true)?;
                layout.artifact_names = objects.names(MAX_ARTIFACTS + MAX_STAGING_FILES)?;
                let mut published_count = 0;
                for object in &layout.artifact_names {
                    self.check()?;
                    if !artifact_name(object) && !temporary_name(object) {
                        return Err(StoreError::new("store_unsafe_entry", false));
                    }
                    published_count += usize::from(artifact_name(object));
                    if published_count > MAX_ARTIFACTS {
                        return Err(StoreError::new("store_layout_limit", false));
                    }
                    let file = objects.open_file(object, false)?;
                    let length = file.metadata().map_err(|e| io_error(e, false))?.len();
                    if length > MAX_RESULT_BYTES as u64 {
                        return Err(StoreError::new("store_object_too_large", false));
                    }
                    layout.bytes = layout.bytes.saturating_add(length);
                }
            } else if RUN_METADATA.contains(&name.as_str()) || temporary_name(name) {
                let bytes = fs::read(&run, name, MAX_METADATA_BYTES)?;
                layout.bytes = layout.bytes.saturating_add(bytes.len() as u64);
                if RUN_METADATA.contains(&name.as_str())
                    && let Err(error) = version(&bytes)
                    && layout.version_error != Some("store_unsupported_format")
                {
                    layout.version_error = Some(error.code);
                }
            } else {
                return Err(StoreError::new("store_unsafe_entry", false));
            }
        }
        if journal.is_some() {
            layout.names.push("deletion.json".into());
        }
        Ok(layout)
    }

    fn writable(&self) -> Result<(), StoreError> {
        self.check()?;
        if let Some(code) = self.mutation_error {
            return Err(StoreError::new(code, false));
        }
        if self.poisoned {
            return Err(StoreError::new("store_unresolved", true));
        }
        Ok(())
    }

    fn write_atomic(
        &mut self,
        directory: &Directory,
        name: &str,
        bytes: &[u8],
        replace: bool,
        budget: Option<(u64, u64)>,
    ) -> Result<(), StoreError> {
        self.writable()?;
        let previous = if replace {
            directory
                .open_file(name, false)?
                .metadata()
                .map_err(|e| io_error(e, false))?
                .len()
        } else {
            match directory.open_file(name, false) {
                Ok(_) => return Err(StoreError::new("store_collision", false)),
                Err(error) if error.code == "store_not_found" => 0,
                Err(error) => return Err(error),
            }
        };
        let peak = self.used_bytes.saturating_add(bytes.len() as u64);
        if peak > self.options.max_bytes {
            return Err(StoreError::new("store_quota_exceeded", false));
        }
        if budget.is_some_and(|(initial, reservation)| peak.saturating_sub(initial) > reservation) {
            return Err(StoreError::new("store_reservation_exceeded", false));
        }
        let temporary = temporary();
        let mut file_created = false;
        let written: Result<(), StoreError> = (|| {
            self.check()?;
            let mut file = directory.create_file(&temporary)?;
            file_created = true;
            if budget.is_none() {
                self.management_effects = true;
            }
            #[cfg(test)]
            if let Err(error) = fail_at(FailurePoint::Write) {
                file.write_all(&bytes[..bytes.len() / 2])
                    .map_err(|e| io_error(e, true))?;
                return Err(error);
            }
            file.write_all(bytes).map_err(|e| io_error(e, true))?;
            self.check().map_err(StoreError::after_effects)?;
            #[cfg(test)]
            fail_at(FailurePoint::FileSync)?;
            file.sync_all().map_err(|e| io_error(e, true))?;
            self.check().map_err(StoreError::after_effects)?;
            #[cfg(test)]
            fail_at(FailurePoint::Publish)?;
            directory.publish(&temporary, name, replace)?;
            #[cfg(test)]
            fail_at(FailurePoint::DirectorySync)?;
            self.check().map_err(StoreError::after_effects)?;
            directory.sync()?;
            Ok(())
        })();
        if let Err(error) = written {
            // Even zero-byte staging files are preserved. A fresh explicit recovery owns repair.
            let effects = file_created || error.effects;
            self.poisoned |= effects;
            if budget.is_none() && effects {
                self.management_effects = true;
            }
            return Err(if effects {
                error.after_effects()
            } else {
                error
            });
        }
        self.used_bytes = peak.saturating_sub(previous);
        Ok(())
    }

    pub(crate) fn begin_collection(
        &mut self,
        id: &str,
        sanitized_intent: Value,
        reservation: u64,
    ) -> Result<CollectionLease<'_>, StoreError> {
        if !valid_run_id(id) {
            return Err(StoreError::new("store_invalid_id", false));
        }
        payload(&sanitized_intent)?;
        if !(1..=MAX_COLLECTION_BYTES).contains(&reservation) {
            return Err(StoreError::new("store_invalid_reservation", false));
        }
        let ids = self.rescan()?;
        self.writable()?;
        if ids.iter().any(|existing| existing == id) {
            return Err(StoreError::new("store_collision", false));
        }
        if ids.len() >= MAX_RUNS {
            return Err(StoreError::new("store_run_limit", false));
        }
        if self.used_bytes.saturating_add(reservation) > self.options.max_bytes {
            return Err(StoreError::new("store_quota_exceeded", false));
        }
        let created_at_unix_ms = now_unix_ms();
        let intent = json_bytes(&Intent {
            format_version: FORMAT_VERSION,
            id: id.into(),
            created_at_unix_ms,
            reservation,
            payload: sanitized_intent.clone(),
        })?;
        let checkpoint = json_bytes(&Checkpoint {
            format_version: FORMAT_VERSION,
            id: id.into(),
            artifacts: BTreeMap::new(),
            payload: sanitized_intent.clone(),
        })?;
        if (intent.len() + checkpoint.len()) as u64 > reservation {
            return Err(StoreError::new("store_reservation_exceeded", false));
        }
        let initial_bytes = self.used_bytes;
        let create: Result<(Directory, Directory), StoreError> = (|| {
            let run = self.runs.create_dir(id)?;
            self.runs.sync()?;
            let objects = run.create_dir("artifacts")?;
            run.sync()?;
            self.write_atomic(
                &run,
                "intent.json",
                &intent,
                false,
                Some((initial_bytes, reservation)),
            )?;
            self.write_atomic(
                &run,
                "checkpoint.json",
                &checkpoint,
                false,
                Some((initial_bytes, reservation)),
            )?;
            Ok((run, objects))
        })();
        let (run, objects) = match create {
            Ok(directories) => directories,
            Err(error) => {
                self.poisoned = true;
                return Err(error.after_effects());
            }
        };
        Ok(CollectionLease {
            store: self,
            run,
            objects,
            id: id.into(),
            created_at_unix_ms,
            initial_bytes,
            reservation,
            artifacts: BTreeMap::new(),
            payload: sanitized_intent,
            sealed: false,
        })
    }

    fn intent(&self, run: &Directory, id: &str) -> Result<Intent, StoreError> {
        self.check()?;
        let intent: Intent = read_json(run, "intent.json")?;
        if intent.id != id || !(1..=MAX_COLLECTION_BYTES).contains(&intent.reservation) {
            return Err(StoreError::new("store_corrupt", false));
        }
        payload(&intent.payload).map_err(|_| StoreError::new("store_corrupt", false))?;
        Ok(intent)
    }

    fn checkpoint(&self, run: &Directory, id: &str) -> Result<Checkpoint, StoreError> {
        self.check()?;
        let checkpoint: Checkpoint = read_json(run, "checkpoint.json")?;
        if checkpoint.id != id {
            return Err(StoreError::new("store_corrupt", false));
        }
        validate_refs(&checkpoint.artifacts)?;
        payload(&checkpoint.payload).map_err(|_| StoreError::new("store_corrupt", false))?;
        Ok(checkpoint)
    }

    fn protection(&self, run: &Directory, id: &str) -> Result<bool, StoreError> {
        self.check()?;
        let protection: Protection = match read_json(run, "protection.json") {
            Ok(protection) => protection,
            Err(error) if error.code == "store_not_found" => return Ok(false),
            Err(error) => return Err(error),
        };
        if protection.id != id {
            return Err(StoreError::new("store_corrupt", false));
        }
        Ok(protection.protected)
    }

    pub(crate) fn load_verified(&self, id: &str) -> Result<VerifiedBundle, StoreError> {
        self.load_manifest(id, false)
    }

    // Explicit deletion may discard additional *verified* receipts, but readers and retention
    // must never treat those uncommitted observations as a valid sealed bundle.
    pub(crate) fn deletion_candidate(&self, id: &str) -> Result<BundleRef, StoreError> {
        Ok(self.load_manifest(id, true)?.reference)
    }

    fn load_manifest(&self, id: &str, deleting: bool) -> Result<VerifiedBundle, StoreError> {
        let layout = self.layout(id)?;
        if let Some(code) = layout.version_error {
            return Err(StoreError::new(code, false));
        }
        if layout.has("deletion.json") {
            return Err(StoreError::new("store_deleting", false));
        }
        if !layout.has("manifest.json") {
            return Err(StoreError::new("store_incomplete", false));
        }
        let run = self.runs.open_at(id, true)?;
        let manifest_bytes = fs::read(&run, "manifest.json", MAX_METADATA_BYTES)?;
        let manifest: Manifest = decode(&manifest_bytes)?;
        let intent = self.intent(&run, id)?;
        let checkpoint = self.checkpoint(&run, id)?;
        if manifest.id != id
            || !matches!(manifest.state.as_str(), "sealed" | "interrupted")
            || manifest.created_at_unix_ms != intent.created_at_unix_ms
            || manifest.finalized_at_unix_ms < manifest.created_at_unix_ms
            || manifest.artifacts != checkpoint.artifacts
            || manifest.payload != checkpoint.payload
        {
            return Err(StoreError::new("store_corrupt", false));
        }
        payload(&manifest.payload).map_err(|_| StoreError::new("store_corrupt", false))?;
        validate_refs(&manifest.artifacts)?;
        let objects = run.open_at("artifacts", true)?;
        for name in &layout.artifact_names {
            self.check()?;
            if temporary_name(name)
                || manifest.artifacts.values().any(|reference| {
                    reference.path.strip_prefix("artifacts/") == Some(name.as_str())
                })
            {
                continue;
            }
            if !deleting {
                return Err(StoreError::new("store_unreferenced_artifact", false));
            }
            let bytes = fs::read(&objects, name, MAX_RESULT_BYTES)?;
            validate_receipt(&bytes)?;
            if artifact_ref(&bytes).path != format!("artifacts/{name}") {
                return Err(StoreError::new("store_corrupt", false));
            }
        }
        let mut artifacts = BTreeMap::new();
        for (step, reference) in &manifest.artifacts {
            self.check()?;
            artifacts.insert(step.clone(), verify_artifact(&objects, reference)?);
        }
        let bundle = VerifiedBundle {
            reference: BundleRef {
                id: id.into(),
                digest: format!("sha256:{}", sha256(&manifest_bytes)),
            },
            protected: self.protection(&run, id)?,
            manifest,
            artifacts,
        };
        self.validate_bundle(&bundle)?;
        Ok(bundle)
    }

    pub(crate) fn list(&self) -> Result<Vec<RunSummary>, StoreError> {
        let mut summaries = Vec::new();
        for id in self.identifiers()? {
            let layout = self.layout(&id)?;
            if layout.has("deletion.json") {
                let reference = self.deletion_reference(&id);
                if let Err(error) = &reference
                    && matches!(error.code, "store_cancelled" | "store_deadline_exceeded")
                {
                    return Err(*error);
                }
                let protected = match self.runs.open_at(&id, true) {
                    Ok(run) => self.protection(&run, &id).unwrap_or(true),
                    Err(_) => false,
                };
                summaries.push(RunSummary {
                    id,
                    state: if reference.is_ok() {
                        "incomplete"
                    } else {
                        "corrupt"
                    },
                    created_at_unix_ms: None,
                    finalized_at_unix_ms: None,
                    protected,
                    bytes: layout.bytes,
                    error: Some(reference.err().map_or("store_deleting", |error| error.code)),
                });
                continue;
            }
            let run = self.runs.open_at(&id, true)?;
            let protected = self.protection(&run, &id);
            let mut summary = RunSummary {
                id: id.clone(),
                state: "corrupt",
                created_at_unix_ms: None,
                finalized_at_unix_ms: None,
                protected: protected.as_ref().copied().unwrap_or(true),
                bytes: layout.bytes,
                error: None,
            };
            match self.pre_intent(&id, &layout) {
                Ok(true) => {
                    summary.state = "incomplete";
                    summary.error = Some("store_incomplete");
                    summaries.push(summary);
                    continue;
                }
                Err(error)
                    if matches!(error.code, "store_cancelled" | "store_deadline_exceeded") =>
                {
                    return Err(error);
                }
                Err(error) => {
                    summary.error = Some(error.code);
                    summaries.push(summary);
                    continue;
                }
                Ok(false) => {}
            }
            match self.load_verified(&id) {
                Ok(bundle) => {
                    summary.state = if bundle.manifest.state == "sealed" {
                        "sealed"
                    } else {
                        "interrupted"
                    };
                    summary.created_at_unix_ms = Some(bundle.manifest.created_at_unix_ms);
                    summary.finalized_at_unix_ms = Some(bundle.manifest.finalized_at_unix_ms);
                    if layout.unresolved() {
                        summary.error = Some("store_unresolved");
                    }
                }
                Err(error)
                    if matches!(error.code, "store_cancelled" | "store_deadline_exceeded") =>
                {
                    return Err(error);
                }
                Err(error) if error.code == "store_incomplete" => match self
                    .preview_recovery(&id, &layout)
                {
                    Ok(bundle) => {
                        summary.state = "incomplete";
                        summary.created_at_unix_ms = Some(bundle.manifest.created_at_unix_ms);
                        summary.error = Some("store_incomplete");
                    }
                    Err(error)
                        if matches!(error.code, "store_cancelled" | "store_deadline_exceeded") =>
                    {
                        return Err(error);
                    }
                    Err(error) => summary.error = Some(error.code),
                },
                Err(error) => summary.error = Some(error.code),
            }
            if let Err(error) = protected {
                summary.state = "corrupt";
                summary.error = Some(error.code);
            }
            summaries.push(summary);
        }
        Ok(summaries)
    }

    fn preview_recovery(&self, id: &str, layout: &RunLayout) -> Result<VerifiedBundle, StoreError> {
        self.check()?;
        if let Some(code) = layout.version_error {
            return Err(StoreError::new(code, false));
        }
        let run = self.runs.open_at(id, true)?;
        let intent = self.intent(&run, id)?;
        let protected = self.protection(&run, id)?;
        let (mut references, payload) = if layout.has("checkpoint.json") {
            let checkpoint = self.checkpoint(&run, id)?;
            (checkpoint.artifacts, checkpoint.payload)
        } else {
            (BTreeMap::new(), intent.payload)
        };
        self.orphan_refs(&run, layout, &mut references)?;
        let objects = run.open_at("artifacts", true)?;
        let mut artifacts = BTreeMap::new();
        for (step, reference) in &references {
            self.check()?;
            artifacts.insert(step.clone(), verify_artifact(&objects, reference)?);
        }
        let manifest = Manifest {
            format_version: FORMAT_VERSION,
            id: id.into(),
            state: "interrupted".into(),
            created_at_unix_ms: intent.created_at_unix_ms,
            finalized_at_unix_ms: now_unix_ms().max(intent.created_at_unix_ms),
            artifacts: references,
            payload,
        };
        let bundle = VerifiedBundle {
            reference: BundleRef {
                id: id.into(),
                digest: format!("sha256:{}", sha256(&json_bytes(&manifest)?)),
            },
            manifest,
            artifacts,
            protected,
        };
        self.validate_bundle(&bundle)?;
        Ok(bundle)
    }

    fn pre_intent(&self, id: &str, layout: &RunLayout) -> Result<bool, StoreError> {
        // begin_collection cannot publish receipts/checkpoints before its intent. Accept only
        // prefixes of that initial construction, including a partial first temporary write.
        if layout.version_error.is_some()
            || layout
                .names
                .iter()
                .any(|name| name != "artifacts" && !temporary_name(name))
            || !layout.artifact_names.is_empty()
            || layout
                .names
                .iter()
                .filter(|name| temporary_name(name))
                .count()
                > 1
        {
            return Ok(false);
        }
        let run = self.runs.open_at(id, true)?;
        for name in layout.names.iter().filter(|name| temporary_name(name)) {
            self.check()?;
            let bytes = fs::read(&run, name, MAX_METADATA_BYTES)?;
            // Interrupted writes can be truncated. A complete future-format object is not
            // our interrupted intent and must survive explicit recovery unchanged.
            if !layout.has("artifacts") {
                return Ok(false);
            }
            if parse_bounded_json(&bytes, 40).is_ok() {
                let intent: Intent = decode(&bytes)?;
                if intent.id != id || !(1..=MAX_COLLECTION_BYTES).contains(&intent.reservation) {
                    return Err(StoreError::new("store_corrupt", false));
                }
                payload(&intent.payload).map_err(|_| StoreError::new("store_corrupt", false))?;
            }
        }
        Ok(true)
    }

    fn delete_pre_intent(&mut self, id: &str, layout: &RunLayout) -> Result<(), StoreError> {
        let run = self.runs.open_at(id, true)?;
        // No committed identity or observation exists to journal. Each removal prefix remains
        // a recognizable pre-intent layout, so cancellation/restart can resume safely.
        for name in layout.names.iter().filter(|name| temporary_name(name)) {
            self.check()?;
            run.open_file(name, false)?;
            run.unlink(name, false)?;
            self.management_effects = true;
        }
        self.check()?;
        #[cfg(test)]
        fail_at(FailurePoint::DirectorySync)?;
        run.sync()?;
        if layout.has("artifacts") {
            self.check()?;
            run.unlink("artifacts", true)?;
            self.management_effects = true;
            self.check()?;
            #[cfg(test)]
            fail_at(FailurePoint::DirectorySync)?;
            run.sync()?;
        }
        self.check()?;
        self.runs.unlink(id, true)?;
        self.management_effects = true;
        self.check()?;
        self.runs.sync()?;
        self.used_bytes = self.used_bytes.saturating_sub(layout.bytes);
        self.progress.deleted.push(id.into());
        #[cfg(test)]
        self.cancel_after_progress();
        self.check()
    }

    pub(crate) fn protect(&mut self, id: &str, protected: bool) -> Result<(), StoreError> {
        self.rescan()?;
        self.writable()?;
        self.load_verified(id)?;
        let run = self.runs.open_at(id, true)?;
        let exists = match run.open_file("protection.json", false) {
            Ok(_) => true,
            Err(error) if error.code == "store_not_found" => false,
            Err(error) => return Err(error),
        };
        let bytes = json_bytes(&Protection {
            format_version: FORMAT_VERSION,
            id: id.into(),
            protected,
        })?;
        self.write_atomic(&run, "protection.json", &bytes, exists, None)
    }

    fn orphan_refs(
        &self,
        run: &Directory,
        layout: &RunLayout,
        references: &mut BTreeMap<String, ArtifactRef>,
    ) -> Result<(), StoreError> {
        let objects = run.open_at("artifacts", true)?;
        for reference in references.values() {
            self.check()?;
            verify_artifact(&objects, reference)?;
        }
        for name in &layout.artifact_names {
            self.check()?;
            if temporary_name(name) {
                continue;
            }
            let bytes = fs::read(&objects, name, MAX_RESULT_BYTES)?;
            validate_receipt(&bytes)?;
            let reference = artifact_ref(&bytes);
            if reference.path != format!("artifacts/{name}") {
                return Err(StoreError::new("store_corrupt", false));
            }
            if !references
                .values()
                .any(|existing| existing.path == reference.path)
            {
                let key = format!(
                    "recovered-{}",
                    reference.digest.trim_start_matches("sha256:")
                );
                references.insert(key, reference);
            }
        }
        validate_refs(references)
    }

    pub(crate) fn recover(&mut self) -> Result<Vec<BundleRef>, StoreError> {
        let result = self.recover_inner();
        result.map_err(|error| self.management_error(error))
    }

    fn recover_inner(&mut self) -> Result<Vec<BundleRef>, StoreError> {
        let ids = self.rescan()?;
        self.writable()?;
        // Validate all candidates before touching the first one. Failed preparations retain bytes.
        let mut candidates = Vec::new();
        let mut abandoned = Vec::new();
        for id in ids {
            let layout = self.layout(&id)?;
            if let Some(code) = layout.version_error {
                return Err(StoreError::new(code, false));
            }
            if layout.has("deletion.json") {
                return Err(StoreError::new("store_deleting", false));
            }
            if layout.has("manifest.json") {
                self.load_verified(&id)?;
                continue;
            }
            if self.pre_intent(&id, &layout)? {
                abandoned.push((id, layout));
                continue;
            }
            let preview = self.preview_recovery(&id, &layout)?;
            candidates.push((preview.manifest, layout.has("checkpoint.json")));
        }
        let mut recovered = Vec::new();
        for (id, layout) in abandoned {
            self.delete_pre_intent(&id, &layout)?;
        }
        for (manifest, checkpoint_exists) in candidates {
            self.check()?;
            let id = manifest.id.clone();
            let run = self.runs.open_at(&id, true)?;
            let checkpoint = json_bytes(&Checkpoint {
                format_version: FORMAT_VERSION,
                id: id.clone(),
                artifacts: manifest.artifacts.clone(),
                payload: manifest.payload.clone(),
            })?;
            self.write_atomic(
                &run,
                "checkpoint.json",
                &checkpoint,
                checkpoint_exists,
                None,
            )
            .map_err(|error| {
                if recovered.is_empty() {
                    error
                } else {
                    error.after_effects()
                }
            })?;
            let manifest = json_bytes(&manifest)?;
            self.write_atomic(&run, "manifest.json", &manifest, false, None)
                .map_err(StoreError::after_effects)?;
            let reference = BundleRef {
                id,
                digest: format!("sha256:{}", sha256(&manifest)),
            };
            self.progress.recovered.push(reference.clone());
            recovered.push(reference);
            #[cfg(test)]
            self.cancel_after_progress();
            self.check()?;
        }
        Ok(recovered)
    }

    pub(crate) fn delete(&mut self, id: &str) -> Result<(), StoreError> {
        let result = self.delete_inner(id);
        result.map_err(|error| self.management_error(error))
    }

    fn delete_inner(&mut self, id: &str) -> Result<(), StoreError> {
        self.rescan()?;
        self.writable()?;
        let layout = self.layout(id)?;
        if let Some(code) = layout.version_error {
            return Err(StoreError::new(code, false));
        }
        if self.pre_intent(id, &layout)? {
            return self.delete_pre_intent(id, &layout);
        }
        let existing_journal = self.root_journal(id)?;
        let run = match self.runs.open_at(id, true) {
            Ok(run) => Some(run),
            Err(error) if error.code == "store_not_found" && existing_journal.is_some() => None,
            Err(error) => return Err(error),
        };
        if let Some(run) = &run
            && self.protection(run, id)?
        {
            return Err(StoreError::new("store_protected", false));
        }
        // The marker is an atomically renamed validated manifest/intent, so deletion can be
        // retried even at the quota limit and even after some referenced files were removed.
        let deleting = layout.has("deletion.json");
        if deleting {
            let marker = match &existing_journal {
                Some(bytes) => bytes.clone(),
                None => fs::read(
                    run.as_ref()
                        .ok_or(StoreError::new("store_corrupt", false))?,
                    "deletion.json",
                    MAX_METADATA_BYTES,
                )?,
            };
            let value = version(&marker)?;
            if value["id"] != id {
                return Err(StoreError::new("store_corrupt", false));
            }
            if value.get("state").is_some() {
                self.deletion_reference(id)?;
            } else {
                if self.validator.is_some() {
                    return Err(StoreError::new("store_corrupt", false));
                }
                let intent: Intent = decode(&marker)?;
                if !(1..=MAX_COLLECTION_BYTES).contains(&intent.reservation) {
                    return Err(StoreError::new("store_corrupt", false));
                }
                payload(&intent.payload).map_err(|_| StoreError::new("store_corrupt", false))?;
            }
        } else if layout.has("manifest.json") {
            self.deletion_candidate(id)?;
        } else {
            self.preview_recovery(id, &layout)?;
            let run = run
                .as_ref()
                .ok_or(StoreError::new("store_corrupt", false))?;
            self.intent(run, id)?;
            if layout.has("checkpoint.json") {
                self.checkpoint(run, id)?;
            }
        }
        // Every still-present object must be owned, regular, bounded and correctly addressed.
        if layout.has("artifacts") {
            let run = run
                .as_ref()
                .ok_or(StoreError::new("store_corrupt", false))?;
            let objects = run.open_at("artifacts", true)?;
            for name in &layout.artifact_names {
                self.check()?;
                if !temporary_name(name) {
                    let bytes = fs::read(&objects, name, MAX_RESULT_BYTES)?;
                    if artifact_ref(&bytes).path != format!("artifacts/{name}") {
                        return Err(StoreError::new("store_corrupt", false));
                    }
                    validate_receipt(&bytes)?;
                }
            }
        }
        self.check()?;
        let result: Result<(), StoreError> = (|| {
            if let Some(run) = &run {
                if !deleting {
                    let source = if layout.has("manifest.json") {
                        "manifest.json"
                    } else {
                        "intent.json"
                    };
                    self.check()?;
                    run.publish(source, "deletion.json", false)?;
                    self.management_effects = true;
                    self.check()?;
                    run.sync()?;
                }
                if layout.has("artifacts") {
                    let objects = run.open_at("artifacts", true)?;
                    for name in &layout.artifact_names {
                        self.check().map_err(StoreError::after_effects)?;
                        objects.open_file(name, false)?;
                        self.check()?;
                        objects.unlink(name, false)?;
                        self.management_effects = true;
                    }
                    self.check()?;
                    objects.sync()?;
                    self.check()?;
                    run.unlink("artifacts", true)?;
                    self.management_effects = true;
                    self.check()?;
                    run.sync()?;
                }
                for name in run.names(MAX_STAGING_FILES + 6)? {
                    if name == "deletion.json" {
                        continue;
                    }
                    self.check().map_err(StoreError::after_effects)?;
                    if !RUN_METADATA.contains(&name.as_str()) && !temporary_name(&name) {
                        return Err(StoreError::new("store_unsafe_entry", true));
                    }
                    run.open_file(&name, false)?;
                    self.check()?;
                    run.unlink(&name, false)?;
                    self.management_effects = true;
                }
                self.check()?;
                run.sync()?;
                if existing_journal.is_none() {
                    // Preserve the original manifest outside the run until removal of the run
                    // directory is durable. Rename adds no quota pressure and follows no links.
                    self.check()?;
                    run.move_to("deletion.json", &self.root, &journal_name(id))?;
                    self.management_effects = true;
                    self.check()?;
                    self.root.sync()?;
                    self.check()?;
                    run.sync()?;
                }
                #[cfg(test)]
                fail_at(FailurePoint::BeforeRunRemoval)?;
                self.check()?;
                self.runs.unlink(id, true)?;
                self.management_effects = true;
            }
            self.check()?;
            self.runs.sync()?;
            #[cfg(test)]
            fail_at(FailurePoint::AfterRunRemoval)?;
            self.check()?;
            self.root.unlink(&journal_name(id), false)?;
            self.management_effects = true;
            self.check()?;
            self.root.sync()?;
            Ok(())
        })();
        if let Err(error) = result {
            self.management_effects |= error.effects;
            self.poisoned |= self.management_effects;
            return Err(self.management_error(error));
        }
        self.used_bytes = self.used_bytes.saturating_sub(layout.bytes).saturating_sub(
            existing_journal
                .as_ref()
                .map_or(0, |bytes| bytes.len() as u64),
        );
        self.progress.deleted.push(id.into());
        #[cfg(test)]
        self.cancel_after_progress();
        self.check()?;
        Ok(())
    }

    pub(crate) fn deletion_reference(&self, id: &str) -> Result<BundleRef, StoreError> {
        let layout = self.layout(id)?;
        if let Some(code) = layout.version_error {
            return Err(StoreError::new(code, false));
        }
        let bytes = match self.root_journal(id)? {
            Some(bytes) => bytes,
            None => fs::read(
                &self.runs.open_at(id, true)?,
                "deletion.json",
                MAX_METADATA_BYTES,
            )?,
        };
        let mut manifest: Manifest = decode(&bytes)?;
        if manifest.id != id
            || !matches!(manifest.state.as_str(), "sealed" | "interrupted")
            || manifest.finalized_at_unix_ms < manifest.created_at_unix_ms
        {
            return Err(StoreError::new("store_corrupt", false));
        }
        validate_refs(&manifest.artifacts)?;
        payload(&manifest.payload).map_err(|_| StoreError::new("store_corrupt", false))?;
        let mut artifacts = BTreeMap::new();
        let protected = match self.runs.open_at(id, true) {
            Ok(run) => {
                match run.open_at("artifacts", true) {
                    Ok(objects) => {
                        for (step, reference) in &manifest.artifacts {
                            self.check()?;
                            match verify_artifact(&objects, reference) {
                                Ok(bytes) => {
                                    artifacts.insert(step.clone(), bytes);
                                }
                                Err(error) if error.code == "store_not_found" => {}
                                Err(error) => return Err(error),
                            }
                        }
                    }
                    Err(error) if error.code == "store_not_found" => {}
                    Err(error) => return Err(error),
                }
                self.protection(&run, id)?
            }
            Err(error) if error.code == "store_not_found" => false,
            Err(error) => return Err(error),
        };
        let reference = BundleRef {
            id: id.into(),
            digest: format!("sha256:{}", sha256(&bytes)),
        };
        // The journal's original bytes and digest stay immutable. Only this private validation
        // preview can omit already-deleted receipts; it is neither published nor returned.
        manifest.state = "deleting".into();
        self.validate_bundle(&VerifiedBundle {
            reference: reference.clone(),
            manifest,
            artifacts,
            protected,
        })?;
        Ok(reference)
    }

    pub(crate) fn prune(&mut self, now_unix_ms: u64) -> Result<Vec<String>, StoreError> {
        let result = self.prune_inner(now_unix_ms);
        result.map_err(|error| self.management_error(error))
    }

    fn prune_inner(&mut self, now_unix_ms: u64) -> Result<Vec<String>, StoreError> {
        self.rescan()?;
        self.writable()?;
        let age = u64::from(self.options.retention_days) * 86_400_000;
        let mut candidates = Vec::new();
        for summary in self.list()? {
            self.check()?;
            if summary.state == "corrupt" {
                return Err(StoreError::new(
                    summary.error.unwrap_or("store_corrupt"),
                    false,
                ));
            }
            if summary.state == "sealed"
                && !summary.protected
                && summary.error.is_none()
                && summary
                    .finalized_at_unix_ms
                    .is_some_and(|finished| now_unix_ms.saturating_sub(finished) >= age)
            {
                let bundle = self.load_verified(&summary.id)?;
                let layout = self.layout(&summary.id)?;
                let referenced: BTreeSet<_> = bundle
                    .manifest
                    .artifacts
                    .values()
                    .map(|reference| reference.path.as_str())
                    .collect();
                if layout
                    .artifact_names
                    .iter()
                    .any(|name| !referenced.contains(format!("artifacts/{name}").as_str()))
                {
                    continue;
                }
                candidates.push(summary.id);
            }
        }
        let mut deleted = Vec::new();
        for id in candidates {
            self.check()?;
            self.delete(&id).map_err(|error| {
                if deleted.is_empty() {
                    error
                } else {
                    error.after_effects()
                }
            })?;
            deleted.push(id);
        }
        Ok(deleted)
    }
}

impl CollectionLease<'_> {
    pub(crate) fn artifacts(&self) -> &BTreeMap<String, ArtifactRef> {
        &self.artifacts
    }

    pub(crate) fn set_deadline(&mut self, deadline: Instant) {
        self.store.deadline = deadline;
    }

    fn active(&self) -> Result<(), StoreError> {
        if self.sealed {
            return Err(StoreError::new("store_collection_closed", false));
        }
        self.store.writable()
    }

    pub(crate) fn put_receipt(
        &mut self,
        id: &str,
        bytes: &[u8],
    ) -> Result<ArtifactRef, StoreError> {
        self.active()?;
        if !step_id(id) || self.artifacts.contains_key(id) || self.artifacts.len() >= MAX_ARTIFACTS
        {
            return Err(StoreError::new("store_invalid_step", false));
        }
        validate_receipt(bytes)?;
        let reference = artifact_ref(bytes);
        let mut next = self.artifacts.clone();
        next.insert(id.into(), reference.clone());
        validate_refs(&next)?;
        let name = reference
            .path
            .strip_prefix("artifacts/")
            .expect("generated artifact path");
        match self.objects.open_file(name, false) {
            Ok(file) => {
                let retained = verify_artifact(&self.objects, &reference)?;
                if retained != bytes {
                    return Err(StoreError::new("store_collision", false));
                }
                file.sync_all().map_err(|e| io_error(e, true))?;
                self.objects.sync()?;
            }
            Err(error) if error.code == "store_not_found" => {
                self.store.write_atomic(
                    &self.objects,
                    name,
                    bytes,
                    false,
                    Some((self.initial_bytes, self.reservation)),
                )?;
            }
            Err(error) => return Err(error),
        }
        // The object is now confirmed durable even if the following checkpoint cannot commit.
        self.artifacts = next;
        self.checkpoint(self.payload.clone())
            .map_err(StoreError::after_effects)?;
        Ok(reference)
    }

    pub(crate) fn checkpoint(&mut self, updated_payload: Value) -> Result<(), StoreError> {
        self.active()?;
        payload(&updated_payload)?;
        let checkpoint = json_bytes(&Checkpoint {
            format_version: FORMAT_VERSION,
            id: self.id.clone(),
            artifacts: self.artifacts.clone(),
            payload: updated_payload.clone(),
        })?;
        self.store.write_atomic(
            &self.run,
            "checkpoint.json",
            &checkpoint,
            true,
            Some((self.initial_bytes, self.reservation)),
        )?;
        self.payload = updated_payload;
        Ok(())
    }

    pub(crate) fn seal(&mut self, final_payload: Value) -> Result<BundleRef, StoreError> {
        self.active()?;
        self.checkpoint(final_payload.clone())?;
        let manifest = json_bytes(&Manifest {
            format_version: FORMAT_VERSION,
            id: self.id.clone(),
            state: "sealed".into(),
            created_at_unix_ms: self.created_at_unix_ms,
            finalized_at_unix_ms: now_unix_ms().max(self.created_at_unix_ms),
            artifacts: self.artifacts.clone(),
            payload: final_payload,
        })?;
        self.store
            .write_atomic(
                &self.run,
                "manifest.json",
                &manifest,
                false,
                Some((self.initial_bytes, self.reservation)),
            )
            .map_err(StoreError::after_effects)?;
        self.sealed = true;
        Ok(BundleRef {
            id: self.id.clone(),
            digest: format!("sha256:{}", sha256(&manifest)),
        })
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailurePoint {
    Write,
    FileSync,
    Publish,
    DirectorySync,
    BeforeRunRemoval,
    AfterRunRemoval,
}

#[cfg(test)]
thread_local! {
    static FAILURE: std::cell::Cell<Option<(FailurePoint, usize)>> = const { std::cell::Cell::new(None) };
    static CANCEL_AFTER_PROGRESS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn fail_at(point: FailurePoint) -> Result<(), StoreError> {
    FAILURE.with(|failure| {
        if let Some((selected, countdown)) = failure.get()
            && selected == point
        {
            if countdown == 0 {
                failure.set(None);
                return Err(StoreError::new("store_injected_failure", true));
            }
            failure.set(Some((selected, countdown - 1)));
        }
        Ok(())
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
