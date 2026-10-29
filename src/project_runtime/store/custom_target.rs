use crate::artifacts::planning_store::StoreError;
use crate::core::lifecycle::GateReceipt;
use crate::ui::layout::RuntimeTargetApprovalProof;

use super::ProjectRuntimeStore;
mod artifacts;
use crate::project_runtime::{
    AppTargetV1, ExecutionRecipeV1, ProjectCapabilityManifestV1, add_user_target_override,
};
use artifacts::validate_artifact_spec;

mod approval;
mod provenance;
mod trusted_adapter;
use approval::{
    RuntimeTargetApprovalRecordV1, approval_path,
    custom_target_origin_sha256 as digest_custom_target_origin,
};

impl<'a> ProjectRuntimeStore<'a> {
    /// Adds a manually selected stack-agnostic target with an authenticated
    /// user receipt, validated and persisted atomically with the manifest.
    pub fn add_user_target_and_save(
        &self,
        target: AppTargetV1,
        recipes: Vec<ExecutionRecipeV1>,
        artifact_paths: Vec<String>,
        approval: GateReceipt,
        approval_proof: RuntimeTargetApprovalProof,
        expected_manifest_revision: u64,
    ) -> Result<ProjectCapabilityManifestV1, StoreError> {
        self.add_user_target_with_readiness_and_save(
            target,
            recipes,
            None,
            artifact_paths,
            approval,
            approval_proof,
            expected_manifest_revision,
        )
    }

    /// Adds an approved custom target, receipt, and optional readiness probe
    /// in one revision-fenced store transaction.
    #[allow(clippy::too_many_arguments)]
    pub fn add_user_target_with_readiness_and_save(
        &self,
        mut target: AppTargetV1,
        recipes: Vec<ExecutionRecipeV1>,
        readiness: Option<super::super::ReadinessProbeV1>,
        artifact_paths: Vec<String>,
        approval: GateReceipt,
        approval_proof: RuntimeTargetApprovalProof,
        expected_manifest_revision: u64,
    ) -> Result<ProjectCapabilityManifestV1, StoreError> {
        target.project_relative_root = crate::project_runtime::normalize_custom_target_root(
            target.project_relative_root.trim(),
        )
        .map_err(|error| StoreError::MalformedState(error.into()))?;
        validate_artifact_spec(&target, &artifact_paths)?;
        let next_revision = expected_manifest_revision.checked_add(1).ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest revision is exhausted".into())
        })?;
        validate_readiness(&target, &recipes, readiness.as_ref())
            .map_err(StoreError::MalformedState)?;
        let artifact_spec = target.artifact_spec_ref.as_ref().map(|_| &artifact_paths);
        let origin_sha256 =
            digest_custom_target_origin(&target, &recipes, readiness.as_ref(), artifact_spec)
                .map_err(StoreError::MalformedState)?;
        let target_id = target.target_id.clone();
        let approval_ref = format!("user-approval:{}", approval.receipt_uid);
        let mut manifest = self.load_manifest()?.ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest is missing".into())
        })?;
        approval::validate_approval(
            &approval,
            &approval_proof,
            &manifest,
            &target,
            &recipes,
            readiness.as_ref(),
            artifact_spec,
        )
        .map_err(StoreError::MalformedState)?;
        if let Some(artifact_spec_ref) = target.artifact_spec_ref.as_ref() {
            manifest
                .artifact_specs
                .insert(artifact_spec_ref.clone(), artifact_paths.clone());
        }
        add_user_target_override(
            &mut manifest,
            target.clone(),
            recipes.clone(),
            &approval_ref,
        )
        .map_err(StoreError::MalformedState)?;
        if let Some(probe) = readiness {
            let probe_id = format!("readiness-{}", uuid::Uuid::new_v4().simple());
            let target = manifest
                .candidate_targets
                .iter_mut()
                .find(|target| target.target_id == target_id)
                .ok_or_else(|| StoreError::MalformedState("custom target was not saved".into()))?;
            if let super::super::ReadinessProbeV1::HttpOnApprovedPort { port, .. }
            | super::super::ReadinessProbeV1::LocalServiceHealth { port, .. } = &probe
            {
                target.port_needs = vec![*port];
            }
            target.readiness_probe_ref = Some(probe_id.clone());
            manifest.readiness_probes.insert(probe_id, probe);
        }
        let store_revision = self.store.revision()?;
        let current = self.load_manifest()?;
        let actual = current.as_ref().map_or(0, |saved| saved.revision);
        if actual != expected_manifest_revision {
            return Err(StoreError::StaleRecordRevision {
                path: super::MANIFEST_PATH.into(),
                expected: expected_manifest_revision,
                actual,
            });
        }
        manifest.revision = next_revision;
        manifest.validate().map_err(StoreError::MalformedState)?;
        let record = RuntimeTargetApprovalRecordV1::new(approval, &manifest, &target_id)?;
        let local_binding = record.local_binding(&target);
        let record_path = approval_path(&record.uid);
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| {
            StoreError::MalformedState(format!(
                "project runtime manifest cannot be encoded: {error}"
            ))
        })?;
        let record_bytes = serde_json::to_vec_pretty(&record).map_err(|error| {
            StoreError::MalformedState(format!(
                "runtime target approval cannot be encoded: {error}"
            ))
        })?;
        // Persist operator-local authority first. If a later local write fails,
        // no shared target is visible; orphaned bindings are scoped to this fresh
        // target/receipt and cannot authorize another proposal.
        crate::persistence::runtime_target_approvals::save(&local_binding).map_err(|error| {
            StoreError::MalformedState(format!(
                "operator-local target approval could not be saved: {error}"
            ))
        })?;
        crate::persistence::runtime_target_approvals::save_custom_target_origin(
            &manifest.workspace_id,
            &target.owning_task_id,
            &origin_sha256,
        )
        .map_err(|error| {
            StoreError::MalformedState(format!(
                "operator-local target origin could not be saved: {error}"
            ))
        })?;
        let saved_target = manifest
            .candidate_targets
            .iter()
            .find(|candidate| candidate.target_id == target_id)
            .ok_or_else(|| StoreError::MalformedState("custom target was not saved".into()))?;
        let mut approved_recipes = Vec::new();
        if let Some(recipe_id) = saved_target.build_recipe_ref.as_deref() {
            approved_recipes.push((super::super::RuntimeOperation::Build, recipe_id));
        }
        approved_recipes.extend(
            saved_target
                .test_recipe_refs
                .iter()
                .map(|recipe_id| (super::super::RuntimeOperation::Test, recipe_id.as_str())),
        );
        if let Some(recipe_id) = saved_target.start_recipe_ref.as_deref() {
            approved_recipes.push((super::super::RuntimeOperation::Start, recipe_id));
        }
        for (operation, recipe_id) in approved_recipes {
            let recipe = manifest
                .user_overrides
                .iter()
                .find(|recipe| recipe.recipe_id == recipe_id)
                .ok_or_else(|| {
                    StoreError::MalformedState("custom target recipe was not saved".into())
                })?;
            let binding =
                Self::user_recipe_approval_binding(&manifest, saved_target, operation, recipe)
                    .map_err(StoreError::MalformedState)?;
            crate::persistence::runtime_target_approvals::save(&binding).map_err(|error| {
                StoreError::MalformedState(format!(
                    "operator-local recipe approval could not be saved: {error}"
                ))
            })?;
        }
        self.store.transaction_with_revision_and_record_revisions(
            &[
                (super::MANIFEST_PATH.into(), manifest_bytes),
                (record_path.clone(), record_bytes),
            ],
            Some(&store_revision),
            &[crate::artifacts::planning_store::RecordRevisionCheck {
                path: record_path,
                expected_revision: 0,
            }],
        )?;
        Ok(manifest)
    }

    /// Verifies the stored user receipt against the target and its complete
    /// recipe/readiness proposal before a custom target can execute.
    pub fn verify_user_target_approval(
        &self,
        manifest: &ProjectCapabilityManifestV1,
        target: &AppTargetV1,
    ) -> Result<(), StoreError> {
        let recipe_ids = target_recipe_ids(target);
        let receipt_id = manifest
            .user_overrides
            .iter()
            .filter(|recipe| recipe_ids.contains(&recipe.recipe_id.as_str()))
            .flat_map(|recipe| recipe.evidence_refs.iter())
            .find_map(|evidence| evidence.strip_prefix("user-approval:"))
            .ok_or_else(|| {
                StoreError::MalformedState("custom target approval receipt is missing".into())
            })?;
        let path = approval_path(receipt_id);
        let (record, _) = self
            .store
            .read_record::<RuntimeTargetApprovalRecordV1>(&path)?;
        record.validate_for(manifest, target)?;
        let binding = record.local_binding(target);
        if !crate::persistence::runtime_target_approvals::contains(&binding).map_err(|error| {
            StoreError::MalformedState(format!(
                "operator-local target approval could not be verified: {error}"
            ))
        })? {
            return Err(StoreError::MalformedState(
                "operator-local approval for this custom target is missing or revoked".into(),
            ));
        }
        Ok(())
    }
}

fn target_recipe_ids(target: &AppTargetV1) -> Vec<&str> {
    target
        .build_recipe_ref
        .iter()
        .chain(target.test_recipe_refs.iter())
        .chain(target.start_recipe_ref.iter())
        .map(String::as_str)
        .collect()
}

fn validate_readiness(
    target: &AppTargetV1,
    recipes: &[ExecutionRecipeV1],
    readiness: Option<&super::super::ReadinessProbeV1>,
) -> Result<(), String> {
    use super::super::{AppSurface, ReadinessProbeV1};

    let Some(start_id) = target.start_recipe_ref.as_deref() else {
        return if readiness.is_none() {
            Ok(())
        } else {
            Err("a readiness probe requires a start recipe".into())
        };
    };
    if target.build_recipe_ref.is_none() {
        return Err("a start recipe requires a build recipe for verified artifacts".into());
    }
    let probe = readiness.ok_or_else(|| "a start recipe requires a readiness probe".to_owned())?;
    probe.validate()?;
    match (target.app_surface, probe) {
        (AppSurface::Web, ReadinessProbeV1::HttpOnApprovedPort { .. })
        | (AppSurface::HeadlessService, ReadinessProbeV1::LocalServiceHealth { .. }) => Ok(()),
        (AppSurface::LinuxDesktop, ReadinessProbeV1::ProcessAlive { executable_id }) => {
            let start = recipes
                .iter()
                .find(|recipe| recipe.recipe_id == start_id)
                .ok_or_else(|| "start recipe is missing from custom target approval".to_owned())?;
            if start.executable_id == *executable_id {
                Ok(())
            } else {
                Err("process readiness must identify the approved start executable".into())
            }
        }
        _ => Err("readiness probe does not match the custom target surface".into()),
    }
}

#[cfg(test)]
#[path = "custom_target/tests.rs"]
mod tests;
