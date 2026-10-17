use super::{
    ArtifactRef, BundleRef, BundleValidator, EvidenceStoreOptions, ManagementProgress, RunSummary,
    StoreError, VerifiedBundle,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    marker::PhantomData,
    sync::{Arc, atomic::AtomicUsize},
    time::Instant,
};

pub(crate) struct Store;
pub(crate) struct CollectionLease<'a> {
    _store: PhantomData<&'a mut Store>,
    artifacts: BTreeMap<String, ArtifactRef>,
}

fn unsupported<T>() -> Result<T, StoreError> {
    Err(StoreError::new("store_unsupported_platform", false))
}

impl Store {
    pub(crate) fn open_validated(
        options: &EvidenceStoreOptions,
        _: bool,
        _: Instant,
        _: BundleValidator,
        _: Option<Arc<AtomicUsize>>,
    ) -> Result<Self, StoreError> {
        options.validate()?;
        unsupported()
    }
    pub(crate) fn progress(&self) -> &ManagementProgress {
        static EMPTY: ManagementProgress = ManagementProgress {
            deleted: Vec::new(),
            recovered: Vec::new(),
        };
        &EMPTY
    }
    pub(crate) fn begin_collection(
        &mut self,
        _: &str,
        _: Value,
        _: u64,
    ) -> Result<CollectionLease<'_>, StoreError> {
        unsupported()
    }
    pub(crate) fn load_verified(&self, _: &str) -> Result<VerifiedBundle, StoreError> {
        unsupported()
    }
    pub(crate) fn list(&self) -> Result<Vec<RunSummary>, StoreError> {
        unsupported()
    }
    pub(crate) fn protect(&mut self, _: &str, _: bool) -> Result<(), StoreError> {
        unsupported()
    }
    pub(crate) fn delete(&mut self, _: &str) -> Result<(), StoreError> {
        unsupported()
    }
    pub(crate) fn deletion_reference(&self, _: &str) -> Result<BundleRef, StoreError> {
        unsupported()
    }
    pub(crate) fn deletion_candidate(&self, _: &str) -> Result<BundleRef, StoreError> {
        unsupported()
    }
    pub(crate) fn prune(&mut self, _: u64) -> Result<Vec<String>, StoreError> {
        unsupported()
    }
    pub(crate) fn recover(&mut self) -> Result<Vec<BundleRef>, StoreError> {
        unsupported()
    }
}

impl CollectionLease<'_> {
    pub(crate) fn artifacts(&self) -> &BTreeMap<String, ArtifactRef> {
        &self.artifacts
    }
    pub(crate) fn set_deadline(&mut self, _: Instant) {}
    pub(crate) fn put_receipt(&mut self, _: &str, _: &[u8]) -> Result<ArtifactRef, StoreError> {
        unsupported()
    }
    pub(crate) fn checkpoint(&mut self, _: Value) -> Result<(), StoreError> {
        unsupported()
    }
    pub(crate) fn seal(&mut self, _: Value) -> Result<BundleRef, StoreError> {
        unsupported()
    }
}
