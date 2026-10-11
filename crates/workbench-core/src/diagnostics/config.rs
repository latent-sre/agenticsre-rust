//! Trusted startup-only destination admission. Wire inputs cannot construct a binding.
use super::{check, digest};
use crate::{
    RunControl,
    request::{Problem, parse_bounded_json, valid_operation},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use rustls::{ClientConfig, RootCertStore, pki_types::CertificateDer};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeSet, io::Read, net::IpAddr, path::Path, sync::Arc, time::Instant};

pub(super) const MAX_FILE: usize = 64 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Process {
    pub backend: String,
    pub pid: u32,
    pub start_time_ticks: String,
    pub uid: u32,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Resolver {
    System {
        transport: String,
    },
    Explicit {
        address: String,
        port: u16,
        transport: String,
    },
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Target {
    pub id: String,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub allowed_ips: Vec<String>,
    pub resolver: Resolver,
    pub tls_ca_file: Option<String>,
    pub process: Option<Process>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema_version: u64,
    targets: Vec<Target>,
}

pub(super) struct Prepared {
    pub target: Target,
    pub allowed: BTreeSet<IpAddr>,
    pub tls: Arc<ClientConfig>,
    pub identity: Value,
}

pub(super) fn problem(code: &'static str) -> Problem {
    Problem::invalid(
        code,
        "The diagnostics input or trusted configuration could not be admitted; private file and transport details are withheld.",
    )
}

pub(super) fn ip(raw: &str) -> Option<IpAddr> {
    let address = raw.parse::<IpAddr>().ok()?;
    // Mapped IPv6 can bypass family-based allowlists and is never a second spelling of IPv4 here.
    if matches!(address, IpAddr::V6(value) if value.to_ipv4_mapped().is_some()) {
        return None;
    }
    Some(address)
}

pub(super) fn hostname(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    !raw.is_empty() && raw.len() <= 253 && raw.is_ascii()
        && !["localhost", "invalid", "onion"].iter().any(|suffix| lower == *suffix || lower.ends_with(&format!(".{suffix}")))
        && raw.split('.').all(|label| !label.is_empty() && label.len() <= 63
            && label.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
            && label.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
            && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
        // Reject malformed IP-looking text rather than resolving it as a different name.
        && !raw.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.')
}

fn no_null(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Array(values) => values.iter().all(no_null),
        Value::Object(values) => values.values().all(no_null),
        _ => true,
    }
}

fn parse(bytes: &[u8]) -> Result<Config, Problem> {
    let invalid = || problem("invalid_diagnostics_configuration");
    let value = parse_bounded_json(bytes, 16).map_err(|_| invalid())?;
    if !no_null(&value)
        || !value.is_object()
        || !value["targets"].as_array().is_some_and(|targets| {
            targets.iter().all(|target| {
                target.is_object()
                    && target["resolver"].is_object()
                    && target.get("process").is_none_or(Value::is_object)
            })
        })
    {
        return Err(invalid());
    }
    let mut config: Config = serde_json::from_value(value).map_err(|_| invalid())?;
    if config.schema_version != 1 || config.targets.is_empty() || config.targets.len() > 16 {
        return Err(invalid());
    }
    let mut ids = BTreeSet::new();
    for target in &mut config.targets {
        if !valid_operation(&target.id)
            || !ids.insert(target.id.clone())
            || target.port == 0
            || target.path.len() > 1024
            || !target.path.starts_with('/')
            || target.path.starts_with("//")
            || !target
                .path
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/-._~!$&'()*+,;=:@".contains(&byte))
            || target
                .path
                .split('/')
                .any(|part| matches!(part, "." | ".."))
            || target.allowed_ips.is_empty()
            || target.allowed_ips.len() > 16
        {
            return Err(invalid());
        }
        target.host = match ip(&target.host) {
            Some(address) => address.to_string(),
            None if hostname(&target.host) => target.host.to_ascii_lowercase(),
            _ => return Err(invalid()),
        };
        let mut addresses = BTreeSet::new();
        for raw in &mut target.allowed_ips {
            let address = ip(raw).ok_or_else(invalid)?;
            if !addresses.insert(address) || address.is_unspecified() || address.is_multicast() {
                return Err(invalid());
            }
            *raw = address.to_string();
        }
        let transport = match &mut target.resolver {
            Resolver::System { transport } => transport,
            Resolver::Explicit {
                address,
                port,
                transport,
            } => {
                let parsed = ip(address).ok_or_else(invalid)?;
                if *port == 0 || parsed.is_unspecified() || parsed.is_multicast() {
                    return Err(invalid());
                }
                *address = parsed.to_string();
                transport
            }
        };
        if !matches!(transport.as_str(), "udp" | "tcp")
            || target.tls_ca_file.as_ref().is_some_and(|path| {
                path.len() > 4096
                    || !Path::new(path).is_absolute()
                    || path.chars().any(char::is_control)
            })
            || target.process.as_ref().is_some_and(|process| {
                !valid_operation(&process.backend)
                    || process.pid == 0
                    || process.pid > i32::MAX as u32
                    || process.start_time_ticks.is_empty()
                    || process.start_time_ticks.len() > 20
                    || !process.start_time_ticks.bytes().all(|b| b.is_ascii_digit())
                    || process.start_time_ticks.parse::<u64>().is_err()
                    || (process.start_time_ticks.len() > 1
                        && process.start_time_ticks.starts_with('0'))
            })
        {
            return Err(invalid());
        }
    }
    Ok(config)
}

#[cfg(target_os = "linux")]
pub(super) fn read_file(
    path: &Path,
    control: &RunControl,
    deadline: Instant,
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
    check(control, deadline)?;
    if !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(problem("invalid_diagnostics_file_path"));
    }
    let name = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| problem("invalid_diagnostics_file_path"))?;
    // SAFETY: open_how is an integer-only kernel structure; name and how live for the syscall.
    let mut how: libc::open_how = unsafe { std::mem::zeroed() };
    how.flags = (libc::O_PATH | libc::O_CLOEXEC) as u64;
    how.resolve = libc::RESOLVE_NO_SYMLINKS | libc::RESOLVE_NO_MAGICLINKS;
    // SAFETY: the syscall borrows valid pointers and returns a newly owned fd on success.
    let descriptor = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            libc::AT_FDCWD,
            name.as_ptr(),
            &how,
            std::mem::size_of::<libc::open_how>(),
        )
    } as i32;
    if descriptor < 0 {
        if matches!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ENOSYS | libc::EINVAL)
        ) {
            return Err(Problem::unsupported(
                "diagnostics_descriptor_unavailable",
                "Diagnostics require Linux openat2 and the proc descriptor view; no weaker path admission is used.",
            ));
        }
        return Err(problem("diagnostics_file_read_failed"));
    }
    // SAFETY: descriptor is uniquely owned after the successful openat2 call.
    let pinned = unsafe { File::from_raw_fd(descriptor) };
    let metadata = pinned
        .metadata()
        .map_err(|_| problem("diagnostics_file_read_failed"))?;
    if !metadata.is_file() {
        return Err(problem("invalid_diagnostics_file"));
    }
    if metadata.len() > MAX_FILE as u64 {
        return Err(problem("diagnostics_file_too_large"));
    }
    let mut file = OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(format!("/proc/self/fd/{}", pinned.as_raw_fd()))
        .map_err(|_| Problem::unsupported("diagnostics_descriptor_unavailable", "Diagnostics require a readable pinned regular file and the proc descriptor view; no weaker fallback is used."))?;
    let actual = file
        .metadata()
        .map_err(|_| problem("diagnostics_file_read_failed"))?;
    if !actual.is_file() || actual.ino() != metadata.ino() || actual.dev() != metadata.dev() {
        return Err(problem("diagnostics_file_changed"));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    let mut buffer = [0_u8; 4096];
    loop {
        check(control, deadline)?;
        let length = file
            .read(&mut buffer)
            .map_err(|_| problem("diagnostics_file_read_failed"))?;
        if length == 0 {
            break;
        }
        if bytes.len() + length > MAX_FILE {
            return Err(problem("diagnostics_file_too_large"));
        }
        bytes.extend_from_slice(&buffer[..length]);
    }
    check(control, deadline)?;
    Ok(bytes)
}

#[cfg(not(target_os = "linux"))]
pub(super) fn read_file(
    _path: &Path,
    _control: &RunControl,
    _deadline: Instant,
) -> Result<Vec<u8>, Problem> {
    Err(Problem::unsupported(
        "unsupported_platform",
        "Diagnostics file admission is currently implemented only on Linux.",
    ))
}

fn ca_roots(bytes: &[u8]) -> Result<Vec<CertificateDer<'static>>, Problem> {
    let invalid = || problem("invalid_diagnostics_ca");
    let mut rest = std::str::from_utf8(bytes).map_err(|_| invalid())?.trim();
    let mut certificates = Vec::new();
    while !rest.is_empty() {
        rest = rest
            .strip_prefix("-----BEGIN CERTIFICATE-----")
            .ok_or_else(invalid)?;
        let (encoded, tail) = rest
            .split_once("-----END CERTIFICATE-----")
            .ok_or_else(invalid)?;
        if !encoded
            .bytes()
            .all(|b| b.is_ascii_whitespace() || b.is_ascii_alphanumeric() || b"+/=".contains(&b))
        {
            return Err(invalid());
        }
        let encoded: String = encoded
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();
        let der = STANDARD.decode(encoded).map_err(|_| invalid())?;
        certificates.push(CertificateDer::from(der));
        if certificates.len() > 16 {
            return Err(invalid());
        }
        rest = tail.trim();
    }
    if certificates.is_empty() {
        return Err(invalid());
    }
    Ok(certificates)
}

pub(super) fn load(
    path: Option<&Path>,
    target_id: &str,
    control: &RunControl,
    deadline: Instant,
) -> Result<Prepared, Problem> {
    let path = path.ok_or_else(|| problem("diagnostics_configuration_required"))?;
    let bytes = read_file(path, control, deadline)?;
    let config = parse(&bytes)?;
    let target = config
        .targets
        .into_iter()
        .find(|target| target.id == target_id)
        .ok_or_else(|| problem("diagnostics_target_not_found"))?;
    let mut roots = RootCertStore::empty();
    let bundled = webpki_root_certs::TLS_SERVER_ROOT_CERTS;
    let mut root_identity = Vec::new();
    for certificate in bundled {
        root_identity.extend_from_slice(&(certificate.len() as u64).to_be_bytes());
        root_identity.extend_from_slice(certificate.as_ref());
        roots
            .add(certificate.clone())
            .map_err(|_| problem("diagnostics_tls_unavailable"))?;
    }
    let ca_digest = if let Some(path) = &target.tls_ca_file {
        let ca = read_file(Path::new(path), control, deadline)?;
        for certificate in ca_roots(&ca)? {
            roots
                .add(certificate)
                .map_err(|_| problem("invalid_diagnostics_ca"))?;
        }
        Some(digest(&ca))
    } else {
        None
    };
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let mut tls = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(|_| problem("diagnostics_tls_unavailable"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    tls.enable_sni = true;
    tls.resumption = rustls::client::Resumption::disabled();
    // The default NoKeyLog and fresh client configuration ignore ambient trust/keylog settings.
    let allowed = target
        .allowed_ips
        .iter()
        .map(|raw| ip(raw).expect("validated IP"))
        .collect();
    let identity = json!({"configuration_sha256":digest(&bytes),"additional_ca_sha256":ca_digest,
        "bundled_roots":{"package":"webpki-root-certs","version":"1.0.9","sha256":digest(&root_identity)}});
    check(control, deadline)?;
    Ok(Prepared {
        target,
        allowed,
        tls: Arc::new(tls),
        identity,
    })
}
