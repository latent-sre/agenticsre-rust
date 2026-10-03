//! Offline context declarations. A mapping is data, never a dispatch or grant.
use crate::{
    OperatorContext, RunControl,
    request::{Problem, Request, bounded, parse_bounded_json},
    result::{OperationResult, Status},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, io::Read, path::Path, sync::atomic::Ordering, time::Instant};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const MAX_SOURCE: usize = 256 * 1024;
const MAX_DATA: usize = 64 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Selector {
    service: String,
    environment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deployment: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Source {
    kind: String,
    id: String,
    revision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    schema_version: u64,
    source: Source,
    records: Vec<Record>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Bindings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    organization: Option<String>,
}

impl Bindings {
    fn get(&self, name: &str) -> Option<&str> {
        match name {
            "target" => self.target.as_deref(),
            "environment" => self.environment.as_deref(),
            "organization" => self.organization.as_deref(),
            _ => None,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Resource {
    kind: String,
    alias: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Dependencies {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    forward: Option<Vec<Selector>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reverse: Option<Vec<Selector>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    service: String,
    team: String,
    environment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deployment: Option<String>,
    state: String,
    last_reviewed: String,
    required_bindings: Vec<String>,
    bindings: Bindings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owners: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resource_aliases: Option<Vec<Resource>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runbooks: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dependencies: Option<Dependencies>,
}

fn problem(code: &'static str) -> Problem {
    Problem::invalid(
        code,
        "The offline context source or selectors could not be resolved; no target was dispatched.",
    )
}

fn strings(values: &Option<Vec<String>>, maximum: usize) -> bool {
    values
        .as_ref()
        .is_none_or(|items| items.len() <= 32 && items.iter().all(|item| bounded(item, 1, maximum)))
}

fn selector_valid(value: &Selector) -> bool {
    bounded(&value.service, 1, 128)
        && bounded(&value.environment, 1, 128)
        && value.team.as_ref().is_none_or(|s| bounded(s, 1, 128))
        && value.deployment.as_ref().is_none_or(|s| bounded(s, 1, 128))
}

fn no_null(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Array(items) => items.iter().all(no_null),
        Value::Object(items) => items.values().all(no_null),
        _ => true,
    }
}

fn export_objects(value: &Value) -> bool {
    value.is_object()
        && value.get("source").is_some_and(Value::is_object)
        && value
            .get("records")
            .and_then(Value::as_array)
            .is_some_and(|records| {
                records.iter().all(|record| {
                    record.is_object()
                        && record.get("bindings").is_some_and(Value::is_object)
                        && record.get("resource_aliases").is_none_or(|items| {
                            items
                                .as_array()
                                .is_some_and(|items| items.iter().all(Value::is_object))
                        })
                        && record.get("dependencies").is_none_or(|dependencies| {
                            dependencies.is_object()
                                && ["forward", "reverse"].iter().all(|direction| {
                                    dependencies.get(direction).is_none_or(|items| {
                                        items
                                            .as_array()
                                            .is_some_and(|items| items.iter().all(Value::is_object))
                                    })
                                })
                        })
                })
            })
}

fn review_time(value: &str) -> Result<OffsetDateTime, Problem> {
    static TIMESTAMP: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\A[0-9]{4}-[0-9]{2}-[0-9]{2}[Tt][0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?(?:Z|\+00:00)\z").expect("fixed UTC RFC3339 grammar")
    });
    if value.len() > 64 || !TIMESTAMP.is_match(value) {
        return Err(problem("invalid_context_review_time"));
    }
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| problem("invalid_context_review_time"))
}

fn check(request: &Request, control: &RunControl, start: Instant) -> Result<(), Problem> {
    if control.signal.load(Ordering::Relaxed) != 0 {
        return Err(problem("cancelled"));
    }
    if start.elapsed().as_millis() >= u128::from(request.limits.timeout_ms) {
        return Err(problem("timed_out"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_source(
    path: &Path,
    request: &Request,
    control: &RunControl,
    start: Instant,
) -> Result<Vec<u8>, Problem> {
    use std::{
        ffi::CString,
        fs::{File, OpenOptions},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
        path::Component,
    };
    if !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(problem("invalid_context_source_path"));
    }
    let name = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| problem("invalid_context_source_path"))?;
    // SAFETY: open_how has only integer fields; no pointers are retained by openat2.
    let mut how: libc::open_how = unsafe { std::mem::zeroed() };
    how.flags = (libc::O_PATH | libc::O_CLOEXEC) as u64;
    how.resolve = libc::RESOLVE_NO_SYMLINKS | libc::RESOLVE_NO_MAGICLINKS;
    // SAFETY: name/how remain valid throughout the syscall; a successful fd is newly owned.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            libc::AT_FDCWD,
            name.as_ptr(),
            &how,
            std::mem::size_of::<libc::open_how>(),
        )
    } as i32;
    if fd < 0 {
        if matches!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ENOSYS | libc::EINVAL)
        ) {
            return Err(Problem::unsupported(
                "context_descriptor_unavailable",
                "Context lookup requires Linux openat2 and a usable proc descriptor view; no weaker fallback is used.",
            ));
        }
        return Err(problem("context_source_read_failed"));
    }
    // SAFETY: the successful syscall returned a unique file descriptor.
    let pinned = unsafe { File::from_raw_fd(fd) };
    let metadata = pinned
        .metadata()
        .map_err(|_| problem("context_source_read_failed"))?;
    if !metadata.is_file() {
        return Err(problem("invalid_context_source_file"));
    }
    if metadata.len() > MAX_SOURCE as u64 {
        return Err(problem("context_source_too_large"));
    }
    // Only a confirmed regular inode gets a data descriptor. This path is derived
    // from our live pinned fd, never from source text or the original user pathname.
    let mut file = OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(format!("/proc/self/fd/{}", pinned.as_raw_fd()))
        .map_err(|_| Problem::unsupported("context_descriptor_unavailable", "Context lookup requires a readable pinned regular file and usable proc descriptor view; no weaker fallback is used."))?;
    let actual = file
        .metadata()
        .map_err(|_| problem("context_source_read_failed"))?;
    if !actual.is_file() || actual.dev() != metadata.dev() || actual.ino() != metadata.ino() {
        return Err(problem("context_source_changed"));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    let mut chunk = [0_u8; 4096];
    loop {
        check(request, control, start)?;
        let length = file
            .read(&mut chunk)
            .map_err(|_| problem("context_source_read_failed"))?;
        if length == 0 {
            break;
        }
        if bytes.len() + length > MAX_SOURCE {
            return Err(problem("context_source_too_large"));
        }
        bytes.extend_from_slice(&chunk[..length]);
    }
    Ok(bytes)
}

#[cfg(not(target_os = "linux"))]
fn read_source(
    _path: &Path,
    _request: &Request,
    _control: &RunControl,
    _start: Instant,
) -> Result<Vec<u8>, Problem> {
    Err(Problem::unsupported(
        "unsupported_platform",
        "Offline context descriptor acceptance is currently implemented only on Linux.",
    ))
}

fn resolve(
    bytes: &[u8],
    inputs: &Value,
    days: u16,
    now: OffsetDateTime,
    request: &Request,
    control: &RunControl,
    start: Instant,
) -> Result<Value, Problem> {
    if !inputs.is_object() || !no_null(inputs) {
        return Err(problem("invalid_context_input"));
    }
    let selector: Selector =
        serde_json::from_value(inputs.clone()).map_err(|_| problem("invalid_context_input"))?;
    if !selector_valid(&selector) {
        return Err(problem("invalid_context_input"));
    }
    let value = parse_bounded_json(bytes, 16)?;
    if !export_objects(&value) || !no_null(&value) {
        return Err(problem("invalid_context_source"));
    }
    let export: Export =
        serde_json::from_value(value).map_err(|_| problem("invalid_context_source"))?;
    if export.schema_version != 1 {
        return Err(Problem::unsupported(
            "unsupported_context_source_version",
            "Only context fixture-export version 1 is supported.",
        ));
    }
    if export.source.kind != "fixture-export"
        || !bounded(&export.source.id, 1, 128)
        || !bounded(&export.source.revision, 1, 128)
        || export.records.len() > 128
    {
        return Err(problem("invalid_context_source"));
    }
    let mut matches = Vec::new();
    for record in &export.records {
        check(request, control, start)?;
        if !bounded(&record.last_reviewed, 1, 64) {
            return Err(problem("invalid_context_review_time"));
        }
        let reviewed = review_time(&record.last_reviewed)?;
        if reviewed.offset().whole_seconds() != 0 {
            return Err(problem("invalid_context_review_time"));
        }
        if reviewed > now {
            return Err(problem("future_context_review_time"));
        }
        let valid = bounded(&record.service, 1, 128)
            && bounded(&record.team, 1, 128)
            && bounded(&record.environment, 1, 128)
            && record
                .deployment
                .as_ref()
                .is_none_or(|s| bounded(s, 1, 128))
            && matches!(record.state.as_str(), "active" | "retired")
            && record.required_bindings.len() <= 3
            && record
                .required_bindings
                .iter()
                .all(|s| matches!(s.as_str(), "target" | "environment" | "organization"))
            && record
                .required_bindings
                .iter()
                .collect::<HashSet<_>>()
                .len()
                == record.required_bindings.len()
            && ["target", "environment", "organization"]
                .iter()
                .all(|name| record.bindings.get(name).is_none_or(|s| bounded(s, 0, 128)))
            && record
                .bindings
                .environment
                .as_ref()
                .is_none_or(|s| s == &record.environment)
            && strings(&record.owners, 128)
            && strings(&record.runbooks, 2048)
            && record.resource_aliases.as_ref().is_none_or(|items| {
                items.len() <= 32
                    && items
                        .iter()
                        .all(|s| bounded(&s.kind, 1, 128) && bounded(&s.alias, 1, 128))
            })
            && record.dependencies.as_ref().is_none_or(|deps| {
                [&deps.forward, &deps.reverse].iter().all(|items| {
                    items
                        .as_ref()
                        .is_none_or(|items| items.len() <= 32 && items.iter().all(selector_valid))
                })
            });
        if !valid {
            return Err(problem("invalid_context_source"));
        }
        if record.service == selector.service
            && record.environment == selector.environment
            && selector.team.as_ref().is_none_or(|s| s == &record.team)
            && selector
                .deployment
                .as_ref()
                .is_none_or(|s| record.deployment.as_ref() == Some(s))
        {
            matches.push((record, reviewed));
        }
    }
    if matches.is_empty() {
        return Err(problem("context_not_found"));
    }
    if matches.len() != 1 {
        return Err(problem("context_ambiguous"));
    }
    let (record, reviewed) = matches[0];
    let age = (now - reviewed).whole_seconds();
    let missing_required: Vec<_> = record
        .required_bindings
        .iter()
        .filter(|name| record.bindings.get(name).is_none_or(str::is_empty))
        .collect();
    let mut missing_optional = Vec::new();
    for name in ["target", "environment", "organization"] {
        if !record
            .required_bindings
            .iter()
            .any(|required| required == name)
            && record.bindings.get(name).is_none_or(str::is_empty)
        {
            missing_optional.push(name);
        }
    }
    if record.owners.is_none() {
        missing_optional.push("owners");
    }
    if record.resource_aliases.is_none() {
        missing_optional.push("resource_aliases");
    }
    if record.runbooks.is_none() {
        missing_optional.push("runbooks");
    }
    if record
        .dependencies
        .as_ref()
        .is_none_or(|s| s.forward.is_none())
    {
        missing_optional.push("dependencies.forward");
    }
    if record
        .dependencies
        .as_ref()
        .is_none_or(|s| s.reverse.is_none())
    {
        missing_optional.push("dependencies.reverse");
    }
    let state = if record.state == "retired" {
        "retired"
    } else if now - reviewed > time::Duration::days(i64::from(days)) {
        "stale"
    } else if !missing_required.is_empty() {
        "incomplete_required"
    } else {
        "resolved"
    };
    let digest = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let data = json!({"source":{"kind":export.source.kind,"id":export.source.id,"revision":export.source.revision,
        "sha256":format!("sha256:{digest}"),"observed_at":now.format(&Rfc3339).expect("UTC timestamp"),"evidence_state":"sourced"},
        "record":record,"resolution":{"state":state,"usable":state == "resolved","max_age_days":days,"age_seconds":age,
            "missing_required":missing_required,"missing_optional":missing_optional}});
    if serde_json::to_vec(&data)
        .map_err(|_| problem("invalid_context_source"))?
        .len()
        > MAX_DATA
    {
        return Err(problem("context_output_too_large"));
    }
    check(request, control, start)?;
    Ok(data)
}

pub(crate) fn run(
    request: &Request,
    control: &RunControl,
    context: &OperatorContext,
    start: Instant,
    result: &mut OperationResult,
) {
    let operation = || {
        let path = context
            .context_source_path
            .as_deref()
            .ok_or_else(|| problem("context_source_required"))?;
        let days = context
            .context_max_age_days
            .filter(|days| (1..=365).contains(days))
            .ok_or_else(|| problem("context_age_policy_required"))?;
        // Validate selectors before reading operator-selected source data.
        if !request.inputs.is_object() {
            return Err(problem("invalid_context_input"));
        }
        let selector: Selector = serde_json::from_value(request.inputs.clone())
            .map_err(|_| problem("invalid_context_input"))?;
        if !selector_valid(&selector) || !no_null(&request.inputs) {
            return Err(problem("invalid_context_input"));
        }
        let bytes = read_source(path, request, control, start)?;
        resolve(
            &bytes,
            &request.inputs,
            days,
            OffsetDateTime::now_utc(),
            request,
            control,
            start,
        )
    };
    match operation() {
        Ok(data) => {
            let state = data["resolution"]["state"]
                .as_str()
                .expect("resolution state");
            if state != "resolved" {
                let code = match state {
                    "stale" => "context_stale",
                    "retired" => "context_retired",
                    _ => "context_incomplete_bindings",
                };
                result.reject(problem(code));
            } else {
                result.effect_outcome = "not_applicable".into();
            }
            result.coverage.state = if data["resolution"]["usable"] == true
                && data["resolution"]["missing_optional"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
            {
                "complete"
            } else {
                "partial"
            }
            .into();
            result.coverage.scope = "Only the selected offline fixture-export declaration; no live catalog revision, target, approval or credential was verified.".into();
            result.sources = vec![
                json!({"reference":format!("context-export:{}@{}",data["source"]["id"].as_str().expect("source ID"),data["source"]["revision"].as_str().expect("revision")),"observed_at":data["source"]["observed_at"],"evidence_state":"sourced","untrusted":true}),
            ];
            result.data = data;
        }
        Err(error) => {
            if matches!(error.code, "cancelled" | "timed_out") {
                result.execution.status = if error.code == "cancelled" {
                    Status::Cancelled
                } else {
                    Status::TimedOut
                };
                result.error(error.code, error.message);
                if error.code == "cancelled" {
                    result.data =
                        json!({"cancellation_signal":control.signal.load(Ordering::Relaxed)});
                }
            } else {
                result.reject(error);
            }
        }
    }
}
