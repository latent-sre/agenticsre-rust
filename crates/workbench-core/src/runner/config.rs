use super::*;
use crate::evidence::store::fs::{self, Directory};
use serde::Deserialize;
use std::{collections::BTreeSet, io::Read, os::unix::fs::MetadataExt};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Client {
    pub config_version: u64,
    pub endpoint: String,
    pub server_name: String,
    pub runner_id: String,
    pub server_certificate_sha256: String,
    pub ca_certificate: String,
    pub certificate: String,
    pub private_key: String,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Server {
    pub config_version: u64,
    pub runner_id: String,
    pub listen: String,
    pub state_dir: String,
    pub ca_certificate: String,
    pub certificate: String,
    pub private_key: String,
    pub principals: Vec<Principal>,
    pub targets: Vec<Target>,
    pub grants: Vec<Value>,
    pub extension_state_dir: Option<String>,
    pub extension_policy_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promtool_path: Option<String>,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Principal {
    pub id: String,
    pub certificate_sha256: String,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Target {
    pub id: String,
    pub snapshot: Value,
}
pub(super) fn read(
    path: &Path,
    max: usize,
    control: &RunControl,
    deadline: Instant,
) -> Result<Vec<u8>, RunnerError> {
    if !path.is_absolute() || !path.to_str().is_some_and(|s| text(s, 1024)) {
        return Err("invalid_runner_path".into());
    }
    let mut dir = Directory(
        std::fs::File::open("/").map_err(|_| RunnerError::from("runner_config_unavailable"))?,
    );
    let components = path.components().collect::<Vec<_>>();
    if components.len() < 2 || path.file_name().is_none() {
        return Err("invalid_runner_path".into());
    }
    for component in &components[1..components.len() - 1] {
        let std::path::Component::Normal(name) = component else {
            return Err("invalid_runner_path".into());
        };
        dir = dir
            .open_at(
                name.to_str()
                    .ok_or(RunnerError::from("invalid_runner_path"))?,
                false,
            )
            .map_err(|_| RunnerError::from("runner_config_unavailable"))?;
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or(RunnerError::from("invalid_runner_path"))?;
    let mut file = dir
        .open_file(name, false)
        .map_err(|_| RunnerError::from("runner_config_unavailable"))?;
    let before = file
        .metadata()
        .map_err(|_| RunnerError::from("runner_config_unavailable"))?;
    if before.len() > max as u64 {
        return Err("runner_config_too_large".into());
    }
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        active(control, deadline)?;
        let n = file
            .read(&mut buffer)
            .map_err(|_| RunnerError::from("runner_config_unavailable"))?;
        if n == 0 {
            break;
        }
        if bytes.len() + n > max {
            return Err("runner_config_too_large".into());
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    let after = file
        .metadata()
        .map_err(|_| RunnerError::from("runner_config_unavailable"))?;
    fs::validate_metadata(&after, false, true)
        .map_err(|_| RunnerError::from("runner_config_unavailable"))?;
    if before.len() != after.len()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
        || bytes.len() as u64 != after.len()
    {
        return Err("runner_config_changed".into());
    }
    Ok(bytes)
}
fn value(
    path: &Path,
    control: &RunControl,
    deadline: Instant,
) -> Result<(Value, String), RunnerError> {
    let bytes = read(path, 65536, control, deadline)?;
    let v = crate::request::parse_bounded_json(&bytes, 12)
        .map_err(|_| RunnerError::from("invalid_runner_config"))?;
    if !v.is_object() || has_null(&v) {
        return Err("invalid_runner_config".into());
    }
    Ok((v, hash(&bytes)))
}
fn has_null(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Object(o) => o.values().any(has_null),
        Value::Array(a) => a.iter().any(has_null),
        _ => false,
    }
}
fn material(
    ca: &str,
    cert: &str,
    key: &str,
    c: &RunControl,
    d: Instant,
) -> Result<transport::Material, RunnerError> {
    Ok(transport::Material {
        ca: read(Path::new(ca), 65536, c, d)?,
        certificate: read(Path::new(cert), 65536, c, d)?,
        private_key: read(Path::new(key), 65536, c, d)?,
    })
}
impl Client {
    pub fn material(&self, c: &RunControl, d: Instant) -> Result<transport::Material, RunnerError> {
        material(
            &self.ca_certificate,
            &self.certificate,
            &self.private_key,
            c,
            d,
        )
    }
}
impl Server {
    pub fn material(&self, c: &RunControl, d: Instant) -> Result<transport::Material, RunnerError> {
        material(
            &self.ca_certificate,
            &self.certificate,
            &self.private_key,
            c,
            d,
        )
    }
    pub fn context(&self, path: std::path::PathBuf) -> OperatorContext {
        OperatorContext {
            job_state_dir: Some(path),
            extension_state_dir: self.extension_state_dir.as_ref().map(Into::into),
            extension_policy_path: self.extension_policy_path.as_ref().map(Into::into),
            promtool_path: self.promtool_path.as_ref().map(Into::into),
            ..OperatorContext::default()
        }
    }
}
pub(super) fn client(
    path: &Path,
    c: &RunControl,
    d: Instant,
) -> Result<(Client, String), RunnerError> {
    let (v, digest) = value(path, c, d)?;
    let cfg: Client =
        serde_json::from_value(v).map_err(|_| RunnerError::from("invalid_runner_config"))?;
    if cfg.config_version != 1
        || cfg.endpoint.parse::<std::net::SocketAddr>().is_err()
        || !crate::request::valid_operation(&cfg.runner_id)
        || !text(&cfg.server_name, 253)
        || !pin(&cfg.server_certificate_sha256)
    {
        return Err("invalid_runner_config".into());
    }
    Ok((cfg, digest))
}
pub(super) fn server(
    path: &Path,
    c: &RunControl,
    d: Instant,
) -> Result<(Server, String), RunnerError> {
    let (v, digest) = value(path, c, d)?;
    for field in ["principals", "targets", "grants"] {
        if !v[field]
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_object))
        {
            return Err("invalid_runner_config".into());
        }
    }
    let cfg: Server =
        serde_json::from_value(v).map_err(|_| RunnerError::from("invalid_runner_config"))?;
    if cfg.config_version != 1
        || !crate::request::valid_operation(&cfg.runner_id)
        || cfg.listen.parse::<std::net::SocketAddr>().is_err()
        || !Path::new(&cfg.state_dir).is_absolute()
        || cfg.principals.is_empty()
        || cfg.principals.len() > 16
        || cfg.targets.is_empty()
        || cfg.targets.len() > 16
        || cfg.grants.len() > 64
    {
        return Err("invalid_runner_config".into());
    }
    let mut names = BTreeSet::new();
    let mut pins = BTreeSet::new();
    for p in &cfg.principals {
        if !crate::request::valid_operation(&p.id)
            || !pin(&p.certificate_sha256)
            || !names.insert(&p.id)
            || !pins.insert(&p.certificate_sha256)
        {
            return Err("invalid_runner_config".into());
        }
    }
    if (cfg.changes.is_some() && cfg.promtool_path.is_none())
        || cfg.changes.as_ref().is_some_and(|v| {
            !crate::change::valid_config(
                v,
                &cfg.principals
                    .iter()
                    .map(|p| p.id.clone())
                    .collect::<Vec<_>>(),
            )
        })
        || cfg
            .promtool_path
            .as_ref()
            .is_some_and(|p| !Path::new(p).is_absolute() || !text(p, 1024))
    {
        return Err("invalid_runner_config".into());
    }
    let mut targets = BTreeSet::new();
    for t in &cfg.targets {
        if !crate::request::valid_operation(&t.id)
            || !targets.insert(&t.id)
            || !keys(&t.snapshot, &["name", "environment"])
            || !["name", "environment"]
                .iter()
                .all(|k| t.snapshot[*k].as_str().is_some_and(|s| text(s, 128)))
        {
            return Err("invalid_runner_config".into());
        }
    }
    let mut grants = BTreeSet::new();
    for g in &cfg.grants {
        let task = g["operation"] == "task.run";
        let fields = if task {
            vec![
                "principal",
                "target",
                "operation",
                "operation_version",
                "task",
                "max_runtime_ms",
            ]
        } else {
            vec![
                "principal",
                "target",
                "operation",
                "operation_version",
                "package_sha256",
                "policy_sha256",
                "max_runtime_ms",
            ]
        };
        if !keys(g, &fields)
            || !cfg.principals.iter().any(|p| g["principal"] == p.id)
            || !cfg.targets.iter().any(|t| g["target"] == t.id)
            || g["operation_version"] != 1
            || !g["max_runtime_ms"]
                .as_u64()
                .is_some_and(|n| (1..=3_600_000).contains(&n))
            || !grants.insert(format!(
                "{}:{}:{}",
                g["principal"], g["target"], g["operation"]
            ))
        {
            return Err("invalid_runner_config".into());
        }
        if task {
            if !keys(&g["task"], &["id", "version", "binding_sha256"])
                || g["task"]["id"] != "error-budget"
                || g["task"]["version"] != 1
                || !g["task"]["binding_sha256"].as_str().is_some_and(pin)
            {
                return Err("invalid_runner_config".into());
            }
        } else if !g["operation"]
            .as_str()
            .is_some_and(|s| s.starts_with("ext.") && crate::request::valid_operation(s))
            || !g["package_sha256"].as_str().is_some_and(pin)
            || !g["policy_sha256"].as_str().is_some_and(pin)
            || cfg.extension_state_dir.is_none()
            || cfg.extension_policy_path.is_none()
        {
            return Err("invalid_runner_config".into());
        }
    }
    Ok((cfg, digest))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn root_path_and_cancelled_reads_refuse_without_panic() {
        let c = RunControl::default();
        assert_eq!(
            read(
                Path::new("/"),
                64,
                &c,
                Instant::now() + Duration::from_secs(1)
            )
            .unwrap_err()
            .code,
            "invalid_runner_path"
        );
        assert_eq!(
            read(
                Path::new("relative"),
                64,
                &c,
                Instant::now() + Duration::from_secs(1)
            )
            .unwrap_err()
            .code,
            "invalid_runner_path"
        );
    }
    #[test]
    fn strict_server_rejects_array_principals_and_unknown_grants() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(crate::result::new_id("runner-config-test"));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.join("config");
        std::fs::write(
            &path,
            br#"{"config_version":1,"principals":[["fake","digest"]]}"#,
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            server(
                &path,
                &RunControl::default(),
                Instant::now() + Duration::from_secs(1)
            )
            .err()
            .unwrap()
            .code,
            "invalid_runner_config"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
