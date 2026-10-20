use std::path::Path;

use crate::artifacts::planning_store::{PlanningStore, StoreError};

use super::{DiscoveryContext, ProjectCapabilityManifestV1, discover_manifest};

mod approval;
mod custom_target;
mod headless_evidence;
mod receipts;
mod recipe_approval;

const MANIFEST_PATH: &str = "state/project-runtime.json";
const RECEIPTS_ROOT: &str = "state/runtime-receipts";
const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_RECEIPT_BYTES: usize = 256 * 1024;

/// Versioned runtime metadata stored only in the active planning repository.
pub struct ProjectRuntimeStore<'a> {
    store: &'a PlanningStore,
}

impl<'a> ProjectRuntimeStore<'a> {
    pub fn new(store: &'a PlanningStore) -> Self {
        Self { store }
    }

    pub fn load_manifest(&self) -> Result<Option<ProjectCapabilityManifestV1>, StoreError> {
        let bytes = match self.store.read(MANIFEST_PATH) {
            Ok(bytes) => bytes,
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(StoreError::MalformedState(
                "project runtime manifest exceeds the read limit".into(),
            ));
        }
        let manifest: ProjectCapabilityManifestV1 =
            serde_json::from_slice(&bytes).map_err(|error| {
                StoreError::MalformedState(format!("invalid project runtime manifest: {error}"))
            })?;
        manifest.validate().map_err(StoreError::MalformedState)?;
        Ok(Some(manifest))
    }

    /// Inspects a selected task checkout and persists only advisory target
    /// metadata. The caller supplies the pinned task and source identity.
    pub fn discover_and_save(
        &self,
        project_root: &Path,
        context: &DiscoveryContext,
        expected_manifest_revision: u64,
    ) -> Result<ProjectCapabilityManifestV1, StoreError> {
        let next_revision = expected_manifest_revision.checked_add(1).ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest revision is exhausted".into())
        })?;
        let manifest =
            discover_manifest(project_root, context).map_err(StoreError::MalformedState)?;
        // Save local origin evidence before publishing the shared manifest. A
        // failed later write can leave only inert local records, never a visible
        // discovered target whose provenance was only partially recorded.
        for target in &manifest.candidate_targets {
            let origin_sha = Self::discovered_target_origin_sha256(
                &manifest.workspace_id,
                &manifest.source_sha,
                target,
            )
            .map_err(StoreError::MalformedState)?;
            crate::persistence::runtime_target_approvals::save_discovered_target_origin(
                &manifest.workspace_id,
                &target.owning_task_id,
                &origin_sha,
            )
            .map_err(|error| {
                StoreError::MalformedState(format!(
                    "operator-local discovery provenance could not be saved: {error}"
                ))
            })?;
        }
        self.save_manifest(&manifest, expected_manifest_revision)?;
        let mut saved = manifest;
        saved.revision = next_revision;
        Ok(saved)
    }

    /// Persists the one target the task controller has explicitly selected.
    /// Multiple discovered targets are never routed by source order.
    pub fn select_target_and_save(
        &self,
        task_id: &str,
        target_id: &str,
        expected_manifest_revision: u64,
    ) -> Result<ProjectCapabilityManifestV1, StoreError> {
        let next_revision = expected_manifest_revision.checked_add(1).ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest revision is exhausted".into())
        })?;
        let mut manifest = self.load_manifest()?.ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest is missing".into())
        })?;
        let target = manifest
            .candidate_targets
            .iter()
            .find(|target| target.target_id == target_id)
            .ok_or_else(|| {
                StoreError::MalformedState("selected runtime target is missing".into())
            })?;
        if target.owning_task_id != task_id || target.repository_id != manifest.repository_id {
            return Err(StoreError::MalformedState(
                "selected runtime target is not bound to this task and repository".into(),
            ));
        }
        manifest.selected_target_id = Some(target_id.to_owned());
        self.save_manifest(&manifest, expected_manifest_revision)?;
        manifest.revision = next_revision;
        Ok(manifest)
    }

    /// Saves with both a manifest revision check and a planning-store snapshot fence.
    pub fn save_manifest(
        &self,
        manifest: &ProjectCapabilityManifestV1,
        expected_manifest_revision: u64,
    ) -> Result<String, StoreError> {
        manifest.validate().map_err(StoreError::MalformedState)?;
        let store_revision = self.store.revision()?;
        let current = self.load_manifest()?;
        let actual = current.as_ref().map_or(0, |saved| saved.revision);
        if actual != expected_manifest_revision {
            return Err(StoreError::StaleRecordRevision {
                path: MANIFEST_PATH.into(),
                expected: expected_manifest_revision,
                actual,
            });
        }
        let mut next = manifest.clone();
        next.revision = actual.checked_add(1).ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest revision is exhausted".into())
        })?;
        let encoded = serde_json::to_vec_pretty(&next).map_err(|error| {
            StoreError::MalformedState(format!(
                "project runtime manifest cannot be encoded: {error}"
            ))
        })?;
        self.store
            .transaction_with_revision(&[(MANIFEST_PATH.into(), encoded)], Some(&store_revision))
            .map(|(_, revision)| revision)
    }
}
