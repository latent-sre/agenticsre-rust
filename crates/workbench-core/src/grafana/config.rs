use super::{Error, PROFILE};
use crate::request::{Problem, bounded, parse_bounded_json, valid_operation};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{Certificate, Url, header::HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::OpenOptions, io::Read, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    spec_version: String,
    record: String,
    connections: BTreeMap<String, Connection>,
    state_dir: Option<String>,
    retention_days: Option<u64>,
    store_max_bytes: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Connection {
    origin: String,
    expected_org_id: i64,
    api_profile: String,
    credential_ref: String,
    tls_ca_file: Option<String>,
}

/// Credential-free startup description. Unsupported profile names are not echoed.
#[derive(Clone, Debug, Serialize)]
pub struct GrafanaTargetDescription {
    pub id: String,
    pub api_profile: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GrafanaConfigDescription {
    /// SHA-256 of the exact bounded configuration bytes, before normalization.
    pub digest: String,
    pub targets: Vec<GrafanaTargetDescription>,
}

/// Opaque launcher-owned identity. It contains no credentials, provider values or paths, and
/// cannot be created from wire data. Custom CA digests are keyed by configured target ID.
#[derive(Clone)]
pub struct GrafanaConfigIdentity {
    config_digest: [u8; 32],
    ca_digests: BTreeMap<String, [u8; 32]>,
}

impl std::fmt::Debug for GrafanaConfigIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GrafanaConfigIdentity")
            .finish_non_exhaustive()
    }
}

/// Describe the explicit configuration without consulting credential environment variables or
/// contacting any target. Each of at most 16 custom CA bundles is bounded to 64 KiB and PEM-checked;
/// trust-anchor acceptance remains part of the existing request client's TLS setup.
/// Execution rechecks these identities on the same bytes it uses, without a precheck/reopen race.
pub fn grafana_config_description(
    path: &Path,
) -> Result<(GrafanaConfigDescription, GrafanaConfigIdentity), Problem> {
    describe(path).map_err(|error| Problem::invalid(
        error.code,
        "The Grafana startup configuration or CA bundle could not be admitted; private configuration details are withheld.",
    ))
}

fn describe(path: &Path) -> Result<(GrafanaConfigDescription, GrafanaConfigIdentity), Error> {
    let bytes = read_file(path, 64 * 1024)?;
    let config = parse_config(&bytes)?;
    let config_digest: [u8; 32] = Sha256::digest(&bytes).into();
    let mut ca_digests = BTreeMap::new();
    let mut targets = Vec::new();
    for (id, connection) in config.connections {
        if let Some(path) = connection.tls_ca_file {
            let bytes = read_file(Path::new(&path), 64 * 1024)?;
            ca_roots(&bytes)?;
            ca_digests.insert(id.clone(), Sha256::digest(&bytes).into());
        }
        targets.push(GrafanaTargetDescription {
            id,
            api_profile: (connection.api_profile == PROFILE).then_some(PROFILE),
        });
    }
    Ok((
        GrafanaConfigDescription {
            digest: format!(
                "sha256:{}",
                config_digest
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            ),
            targets,
        },
        GrafanaConfigIdentity {
            config_digest,
            ca_digests,
        },
    ))
}

// Deliberately no Debug/Serialize: provider values and authorization are private.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    origin: String,
    expected_org_id: i64,
    operations: Vec<String>,
    auth: Auth,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Auth {
    Bearer { token: String },
    Basic { username: String, password: String },
}

pub(super) struct Prepared {
    pub origin: String,
    pub organization: i64,
    pub authorization: HeaderValue,
    pub secrets: Vec<String>,
    pub roots: Vec<Certificate>,
}

pub(super) fn read_file(path: &Path, maximum: usize) -> Result<Vec<u8>, Error> {
    if !path.is_absolute() {
        return Err(Error::admission("invalid_configuration"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| Error::admission("configuration_read_failed"))?;
    if !file.metadata().map(|m| m.is_file()).unwrap_or(false) {
        return Err(Error::admission("invalid_configuration"));
    }
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::admission("configuration_read_failed"))?;
    if bytes.len() > maximum {
        return Err(Error::admission("configuration_too_large"));
    }
    Ok(bytes)
}

pub(super) fn origin(raw: &str) -> Result<String, Error> {
    let invalid = || Error::admission("invalid_origin");
    // Validate BEFORE URL parsing: parsers can normalize dot segments and backslashes away.
    if !bounded(raw, 1, 2048)
        || !raw.starts_with("https://")
        || raw.chars().any(|c| c.is_whitespace() || c.is_control())
        || raw.contains(['\\', '%', '?', '#'])
        || raw.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(invalid());
    }
    let url = Url::parse(raw).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port() == Some(0)
        || raw
            .strip_prefix("https://")
            .unwrap_or("")
            .split('/')
            .next()
            .is_some_and(|s| s.contains('@'))
    {
        return Err(invalid());
    }
    let normalized = url.as_str().trim_end_matches('/');
    if !bounded(normalized, 1, 2048) {
        return Err(invalid());
    }
    Ok(normalized.to_owned())
}

fn configured_reference(reference: &str) -> Option<&str> {
    let name = reference.strip_prefix("env:")?;
    if name.len() > 128
        || !name.starts_with("SAVE_GRAFANA_")
        || name.len() == 13
        || !name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        return None;
    }
    Some(name)
}

fn parse_config(bytes: &[u8]) -> Result<Config, Error> {
    let value =
        parse_bounded_json(bytes, 16).map_err(|_| Error::admission("invalid_configuration"))?;
    if !value.is_object()
        || !value["connections"].is_object()
        || ["state_dir", "retention_days", "store_max_bytes"]
            .iter()
            .any(|k| value.get(k).is_some_and(Value::is_null))
        || value["connections"].as_object().is_some_and(|connections| {
            connections
                .values()
                .any(|c| !c.is_object() || c.get("tls_ca_file").is_some_and(Value::is_null))
        })
    {
        return Err(Error::admission("invalid_configuration"));
    }
    let config: Config =
        serde_json::from_value(value).map_err(|_| Error::admission("invalid_configuration"))?;
    if config.spec_version != "0.1"
        || config.record != "never"
        || config.connections.len() > 16
        || config
            .state_dir
            .as_ref()
            .is_some_and(|s| !bounded(s, 1, 1024))
        || config.retention_days.is_some_and(|n| n > 3650)
        || config
            .store_max_bytes
            .is_some_and(|n| !(1_048_576..=1_099_511_627_776).contains(&n))
    {
        return Err(Error::admission("invalid_configuration"));
    }
    for (name, connection) in &config.connections {
        if !valid_operation(name)
            || connection.expected_org_id <= 0
            || configured_reference(&connection.credential_ref).is_none()
            || !bounded(&connection.api_profile, 1, 128)
            || connection
                .tls_ca_file
                .as_ref()
                .is_some_and(|p| !bounded(p, 1, 1024) || !Path::new(p).is_absolute())
        {
            return Err(Error::admission("invalid_configuration"));
        }
        origin(&connection.origin)?;
    }
    Ok(config)
}

pub(super) fn load(
    path: Option<&Path>,
    alias: &str,
    operation: &str,
    expected_identity: Option<&GrafanaConfigIdentity>,
) -> Result<Prepared, Error> {
    let path = path.ok_or_else(|| {
        Error::admission(if expected_identity.is_some() {
            "grafana_configuration_required"
        } else {
            "configuration_required"
        })
    })?;
    let bytes = read_file(path, 64 * 1024).map_err(|error| {
        if expected_identity.is_some() {
            Error::admission("grafana_configuration_changed")
        } else {
            error
        }
    })?;
    if expected_identity
        .is_some_and(|expected| expected.config_digest != <[u8; 32]>::from(Sha256::digest(&bytes)))
    {
        return Err(Error::admission("grafana_configuration_changed"));
    }
    let mut config = parse_config(&bytes)?;
    let connection = config
        .connections
        .remove(alias)
        .ok_or_else(|| Error::admission("unknown_connection"))?;
    if connection.api_profile != PROFILE {
        return Err(Error::unsupported("unsupported_api_profile"));
    }
    let origin = origin(&connection.origin)?;
    // Pinned execution checks the selected CA before credential lookup. Legacy operator callers
    // retain their existing credential-first admission ordering.
    let pinned_roots = expected_identity
        .map(|identity| roots(&connection, alias, Some(identity)))
        .transpose()?;
    let variable = configured_reference(&connection.credential_ref)
        .ok_or_else(|| Error::admission("invalid_credential_reference"))?;
    let secret = std::env::var(variable).map_err(|_| Error::admission("credential_unavailable"))?;
    let (authorization, secrets) =
        credential(&secret, &origin, connection.expected_org_id, operation)?;
    let roots = match pinned_roots {
        Some(roots) => roots,
        None => roots(&connection, alias, None)?,
    };
    Ok(Prepared {
        origin,
        organization: connection.expected_org_id,
        authorization,
        secrets,
        roots,
    })
}

fn ca_roots(bytes: &[u8]) -> Result<Vec<Certificate>, Error> {
    let roots =
        Certificate::from_pem_bundle(bytes).map_err(|_| Error::admission("invalid_ca_bundle"))?;
    if roots.is_empty() {
        return Err(Error::admission("invalid_ca_bundle"));
    }
    Ok(roots)
}

fn roots(
    connection: &Connection,
    alias: &str,
    expected_identity: Option<&GrafanaConfigIdentity>,
) -> Result<Vec<Certificate>, Error> {
    match &connection.tls_ca_file {
        Some(path) => {
            let bytes = read_file(Path::new(path), 64 * 1024).map_err(|error| {
                if expected_identity.is_some() {
                    Error::admission("grafana_ca_changed")
                } else {
                    error
                }
            })?;
            if expected_identity.is_some_and(|identity| {
                identity.ca_digests.get(alias) != Some(&<[u8; 32]>::from(Sha256::digest(&bytes)))
            }) {
                return Err(Error::admission("grafana_ca_changed"));
            }
            ca_roots(&bytes)
        }
        None => webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .map(|der| {
                Certificate::from_der(der.as_ref())
                    .map_err(|_| Error::admission("invalid_ca_bundle"))
            })
            .collect::<Result<Vec<_>, _>>(),
    }
}

fn credential(
    raw: &str,
    expected_origin: &str,
    organization: i64,
    operation: &str,
) -> Result<(HeaderValue, Vec<String>), Error> {
    let invalid = || Error::admission("invalid_credential");
    if raw.len() > 16 * 1024 {
        return Err(invalid());
    }
    let value = parse_bounded_json(raw.as_bytes(), 8).map_err(|_| invalid())?;
    if !value.is_object() || !value["auth"].is_object() {
        return Err(invalid());
    }
    let credential: Credential = serde_json::from_value(value).map_err(|_| invalid())?;
    if origin(&credential.origin).map_err(|_| invalid())? != expected_origin
        || credential.expected_org_id != organization
        || credential.operations.is_empty()
        || credential.operations.len() > 2
        || credential
            .operations
            .iter()
            .any(|op| !matches!(op.as_str(), "grafana.dashboard.get" | "grafana.query"))
        || !credential.operations.iter().any(|op| op == operation)
        || (credential.operations.len() == 2
            && credential.operations[0] == credential.operations[1])
    {
        return Err(Error::admission("credential_scope_mismatch"));
    }
    let valid_secret =
        |s: &str| !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control);
    let (authorization, mut secrets) = match credential.auth {
        Auth::Bearer { token } => {
            if !valid_secret(&token) || !token.is_ascii() {
                return Err(invalid());
            }
            (format!("Bearer {token}"), vec![token])
        }
        Auth::Basic { username, password } => {
            if !valid_secret(&username) || !valid_secret(&password) || username.contains(':') {
                return Err(invalid());
            }
            let pair = format!("{username}:{password}");
            let encoded = STANDARD.encode(&pair);
            (
                format!("Basic {encoded}"),
                vec![username, password, pair, encoded],
            )
        }
    };
    let mut header = HeaderValue::from_str(&authorization).map_err(|_| invalid())?;
    header.set_sensitive(true);
    secrets.push(authorization);
    Ok((header, secrets))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_validation_precedes_normalization_and_preserves_prefix() {
        assert_eq!(
            origin("https://EXAMPLE.invalid:443/grafana/").unwrap(),
            "https://example.invalid/grafana"
        );
        for value in [
            "http://example.invalid",
            "https://u:p@example.invalid",
            "https://@example.invalid",
            "https://example.invalid/%2e",
            "https://example.invalid/a/../b",
            "https://example.invalid?",
            "https://example.invalid/#",
            "https://example.invalid\\other",
            "https://example.invalid:0",
            "https://example.invalid/\n",
        ] {
            assert!(origin(value).is_err(), "{value}");
        }
    }
    #[test]
    fn provider_scope_and_auth_shapes_cannot_be_replaced() {
        let base = serde_json::json!({"origin":"https://example.invalid/grafana","expected_org_id":7,"operations":["grafana.query"],"auth":{"kind":"bearer","token":"synthetic-only"}});
        assert!(
            credential(
                &base.to_string(),
                "https://example.invalid/grafana",
                7,
                "grafana.query"
            )
            .is_ok()
        );
        for (key, value) in [
            ("origin", serde_json::json!("https://other.invalid")),
            ("expected_org_id", serde_json::json!(8)),
            ("operations", serde_json::json!(["grafana.dashboard.get"])),
            ("auth", serde_json::json!(["bearer", "synthetic-only"])),
        ] {
            let mut invalid = base.clone();
            invalid[key] = value;
            assert!(
                credential(
                    &invalid.to_string(),
                    "https://example.invalid/grafana",
                    7,
                    "grafana.query"
                )
                .is_err()
            );
        }
    }
}
