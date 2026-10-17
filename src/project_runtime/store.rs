use std::path::Path;

use crate::artifacts::planning_store::{PlanningStore, StoreError};

use super::{
    AppTargetV1, DiscoveryContext, ExecutionRecipeV1, ProjectCapabilityManifestV1,
    RuntimeOperation, add_user_target_override, apply_user_recipe_override, discover_manifest,
};

mod approval;
mod receipts;

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
        self.save_manifest(&manifest, expected_manifest_revision)?;
        let mut saved = manifest;
        saved.revision = next_revision;
        Ok(saved)
    }

    /// Applies a user-approved recipe to a stored target under the same
    /// revision fence as discovery and saves it atomically.
    pub fn approve_recipe_and_save(
        &self,
        target_id: &str,
        operation: RuntimeOperation,
        recipe: ExecutionRecipeV1,
        expected_manifest_revision: u64,
    ) -> Result<ProjectCapabilityManifestV1, StoreError> {
        self.approve_recipe_with_readiness_and_save(
            target_id,
            operation,
            recipe,
            None,
            expected_manifest_revision,
        )
    }

    /// Saves a user-approved recipe and its optional readiness probe as one
    /// revision-fenced manifest update.
    pub fn approve_recipe_with_readiness_and_save(
        &self,
        target_id: &str,
        operation: RuntimeOperation,
        recipe: ExecutionRecipeV1,
        readiness: Option<super::ReadinessProbeV1>,
        expected_manifest_revision: u64,
    ) -> Result<ProjectCapabilityManifestV1, StoreError> {
        let next_revision = expected_manifest_revision.checked_add(1).ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest revision is exhausted".into())
        })?;
        let mut manifest = self.load_manifest()?.ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest is missing".into())
        })?;
        let recipe_id = recipe.recipe_id.clone();
        apply_user_recipe_override(&mut manifest, target_id, operation, recipe)
            .map_err(StoreError::MalformedState)?;
        if let Some(probe) = readiness {
            probe.validate().map_err(StoreError::MalformedState)?;
            let probe_id = format!("readiness-{}", uuid::Uuid::new_v4().simple());
            let target = manifest
                .candidate_targets
                .iter_mut()
                .find(|target| target.target_id == target_id)
                .ok_or_else(|| {
                    StoreError::MalformedState("selected runtime target is missing".into())
                })?;
            if let super::ReadinessProbeV1::HttpOnApprovedPort { port, .. }
            | super::ReadinessProbeV1::LocalServiceHealth { port, .. } = &probe
            {
                target.port_needs = vec![*port];
            }
            target.readiness_probe_ref = Some(probe_id.clone());
            manifest.readiness_probes.insert(probe_id, probe);
        }
        self.save_manifest(&manifest, expected_manifest_revision)?;
        let target = manifest
            .candidate_targets
            .iter()
            .find(|target| target.target_id == target_id)
            .ok_or_else(|| StoreError::MalformedState("approved target disappeared".into()))?;
        let recipe = manifest
            .user_overrides
            .iter()
            .find(|recipe| recipe.recipe_id == recipe_id)
            .ok_or_else(|| StoreError::MalformedState("approved recipe disappeared".into()))?;
        self.record_recipe_approval(&manifest, target, recipe)?;
        manifest.revision = next_revision;
        Ok(manifest)
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

    /// Adds a manually selected stack-agnostic target and its approved recipes
    /// with a revision check. Authentication of `approval_ref` is performed by
    /// the UI/controller before this method is called.
    pub fn add_user_target_and_save(
        &self,
        target: AppTargetV1,
        recipes: Vec<ExecutionRecipeV1>,
        approval_ref: &str,
        expected_manifest_revision: u64,
    ) -> Result<ProjectCapabilityManifestV1, StoreError> {
        let next_revision = expected_manifest_revision.checked_add(1).ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest revision is exhausted".into())
        })?;
        let mut manifest = self.load_manifest()?.ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest is missing".into())
        })?;
        add_user_target_override(&mut manifest, target, recipes, approval_ref)
            .map_err(StoreError::MalformedState)?;
        self.save_manifest(&manifest, expected_manifest_revision)?;
        let target = manifest
            .candidate_targets
            .last()
            .ok_or_else(|| StoreError::MalformedState("approved target disappeared".into()))?;
        let recipe_ids = target
            .build_recipe_ref
            .iter()
            .chain(target.test_recipe_refs.iter())
            .chain(target.start_recipe_ref.iter())
            .collect::<Vec<_>>();
        for recipe_id in recipe_ids {
            let recipe = manifest
                .user_overrides
                .iter()
                .find(|recipe| &recipe.recipe_id == recipe_id)
                .ok_or_else(|| StoreError::MalformedState("approved recipe disappeared".into()))?;
            self.record_recipe_approval(&manifest, target, recipe)?;
        }
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
