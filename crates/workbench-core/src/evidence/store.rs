//! Bounded local evidence format. Metadata here describes retained bytes, not observations.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

pub const DEFAULT_MAX_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_STORE_BYTES: u64 = DEFAULT_MAX_BYTES;
pub const DEFAULT_RETENTION_DAYS: u16 = 7;
pub(crate) const MAX_COLLECTION_BYTES: u64 = 8 * 1024 * 1024;
pub(crate) const MAX_METADATA_BYTES: usize = 256 * 1024;
pub(crate) const MAX_ARTIFACTS: usize = 16;
pub(crate) const MAX_RUNS: usize = 1024;

/// Trusted startup policy. Wire requests cannot select a directory or change these limits.
#[derive(Clone, Debug)]
pub struct EvidenceStoreOptions {
    pub directory: PathBuf,
    pub max_bytes: u64,
    pub retention_days: u16,
    identity: Option<(u64, u64, u32)>,
}

impl EvidenceStoreOptions {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            max_bytes: DEFAULT_MAX_BYTES,
            retention_days: DEFAULT_RETENTION_DAYS,
            identity: None,
        }
    }

    pub(crate) fn pinned(&self) -> Result<Self, StoreError> {
        #[cfg(target_os = "linux")]
        {
            let store = Store::open_validated(
                self,
                false,
                std::time::Instant::now() + std::time::Duration::from_secs(30),
                super::validate_bundle,
                None,
            )?;
            let mut pinned = self.clone();
            pinned.identity = Some(store.identity()?);
            Ok(pinned)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(StoreError::new("store_unsupported_platform", false))
        }
    }

    pub(crate) fn validate(&self) -> Result<(), StoreError> {
        if !self.directory.is_absolute()
            || !(1..=MAX_STORE_BYTES).contains(&self.max_bytes)
            || !(1..=3650).contains(&self.retention_days)
        {
            return Err(StoreError::new("store_invalid_options", false));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArtifactRef {
    pub path: String,
    pub media_type: String,
    pub bytes: u64,
    pub digest: String,
    pub classification: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BundleRef {
    pub id: String,
    pub digest: String,
}

/// Immutable format-1 envelope. The coordinator independently validates the versioned payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub format_version: u64,
    pub id: String,
    pub state: String,
    pub created_at_unix_ms: u64,
    pub finalized_at_unix_ms: u64,
    pub artifacts: BTreeMap<String, ArtifactRef>,
    pub payload: Value,
}

#[derive(Debug)]
pub(crate) struct VerifiedBundle {
    pub reference: BundleRef,
    pub manifest: Manifest,
    pub artifacts: BTreeMap<String, Vec<u8>>,
    pub protected: bool,
}

pub(crate) type BundleValidator = fn(&VerifiedBundle) -> Result<(), StoreError>;

/// Only completed, synchronized management actions; retained even when a later action fails.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct ManagementProgress {
    pub deleted: Vec<String>,
    pub recovered: Vec<BundleRef>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RunSummary {
    pub id: String,
    pub state: &'static str,
    pub created_at_unix_ms: Option<u64>,
    pub finalized_at_unix_ms: Option<u64>,
    pub protected: bool,
    pub bytes: u64,
    pub error: Option<&'static str>,
}

/// Static, path-free error. `effects` never implies that a failed write was durable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StoreError {
    pub code: &'static str,
    pub effects: bool,
}

impl StoreError {
    pub(crate) const fn new(code: &'static str, effects: bool) -> Self {
        Self { code, effects }
    }

    pub(crate) fn after_effects(mut self) -> Self {
        self.effects = true;
        self
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for StoreError {}

pub(crate) fn valid_run_id(id: &str) -> bool {
    if id.len() > 96 {
        return false;
    }
    let mut parts = id.split('-');
    parts.next() == Some("run")
        && parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
        && parts.next().is_some_and(lower_hex)
        && parts.next().is_some_and(lower_hex)
        && parts.next().is_none()
}

fn lower_hex(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(target_os = "linux")]
pub(crate) mod fs;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::{CollectionLease, Store};

#[cfg(not(target_os = "linux"))]
mod unsupported;
#[cfg(not(target_os = "linux"))]
pub(crate) use unsupported::{CollectionLease, Store};
