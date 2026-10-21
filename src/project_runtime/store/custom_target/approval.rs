use serde::{Deserialize, Serialize};

use crate::ui::layout::RuntimeTargetApprovalProof;
use crate::{
    artifacts::planning_store::StoreError,
    core::lifecycle::{GateActor, GateDecision, GateReceipt},
    project_runtime::{
        AppTargetV1, ExecutionRecipeV1, ProjectCapabilityManifestV1, ReadinessProbeV1,
    },
};

use super::target_recipe_ids;

const ROOT: &str = "state/runtime-target-approvals";
const MAX_RECORD_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RuntimeTargetApprovalRecordV1 {
    pub uid: String,
    pub schema_version: u16,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub revision: u64,
    pub workspace_id: String,
    pub repository_id: String,
    pub source_sha: String,
    pub target_id: String,
    pub approval: GateReceipt,
}

impl RuntimeTargetApprovalRecordV1 {
    pub fn new(
        approval: GateReceipt,
        manifest: &ProjectCapabilityManifestV1,
        target_id: &str,
    ) -> Result<Self, StoreError> {
        let record = Self {
            uid: approval.receipt_uid.clone(),
            schema_version: 1,
            created_at_ms: approval.recorded_at_unix_ms,
            updated_at_ms: approval.recorded_at_unix_ms,
            revision: 1,
            workspace_id: manifest.workspace_id.clone(),
            repository_id: manifest.repository_id.clone(),
            source_sha: manifest.source_sha.clone(),
            target_id: target_id.to_owned(),
            approval,
        };
        if serde_json::to_vec(&record)
            .map_err(|error| StoreError::MalformedState(error.to_string()))?
            .len()
            > MAX_RECORD_BYTES
        {
            return Err(StoreError::MalformedState(
                "runtime target approval record exceeds its write limit".into(),
            ));
        }
        Ok(record)
    }

    pub fn validate_for(
        &self,
        manifest: &ProjectCapabilityManifestV1,
        target: &AppTargetV1,
    ) -> Result<(), StoreError> {
        let ids = target_recipe_ids(target);
        let recipes = manifest
            .user_overrides
            .iter()
            .filter(|recipe| ids.contains(&recipe.recipe_id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let readiness = target
            .readiness_probe_ref
            .as_ref()
            .and_then(|id| manifest.readiness_probes.get(id));
        let artifact_spec = target
            .artifact_spec_ref
            .as_ref()
            .and_then(|id| manifest.artifact_specs.get(id));
        let expected_ref = format!("user-approval:{}", self.uid);
        let expected_evidence = format!("runtime-target:{}", target.target_id);
        let bound_recipes = recipes.iter().all(|recipe| {
            recipe
                .evidence_refs
                .iter()
                .any(|item| item == &expected_ref)
        });
        let expected_sha256 = proposal_sha256(
            &self.workspace_id,
            &self.repository_id,
            &self.source_sha,
            target,
            &recipes,
            readiness,
            artifact_spec,
        )
        .map_err(StoreError::MalformedState)?;
        if self.schema_version != 1
            || self.uid != self.approval.receipt_uid
            || self.revision != 1
            || self.created_at_ms != self.approval.recorded_at_unix_ms
            || self.updated_at_ms != self.created_at_ms
            || self.workspace_id != manifest.workspace_id
            || self.repository_id != manifest.repository_id
            || self.source_sha != manifest.source_sha
            || self.target_id != target.target_id
            || self.approval.actor != GateActor::User
            || self.approval.decision != GateDecision::Approved
            || self.approval.subject_uid != target.owning_task_id
            || !self
                .approval
                .evidence_refs
                .iter()
                .any(|item| item == &expected_evidence)
            || !bound_recipes
            || self.approval.subject_sha256 != expected_sha256
        {
            return Err(StoreError::MalformedState(
                "persisted runtime target approval does not match the selected target".into(),
            ));
        }
        if serde_json::to_vec(self)
            .map_err(|error| StoreError::MalformedState(error.to_string()))?
            .len()
            > MAX_RECORD_BYTES
        {
            return Err(StoreError::MalformedState(
                "runtime target approval record exceeds its read limit".into(),
            ));
        }
        self.approval
            .validate()
            .map_err(StoreError::MalformedState)?;
        Ok(())
    }

    pub fn local_binding(
        &self,
        target: &AppTargetV1,
    ) -> crate::persistence::runtime_target_approvals::Binding {
        crate::persistence::runtime_target_approvals::Binding {
            workspace_id: self.workspace_id.clone(),
            task_id: target.owning_task_id.clone(),
            repository_id: self.repository_id.clone(),
            source_sha: self.source_sha.clone(),
            target_id: self.target_id.clone(),
            proposal_sha256: self.approval.subject_sha256.clone(),
            receipt_id: self.uid.clone(),
        }
    }
}

pub(super) fn validate_approval(
    receipt: &GateReceipt,
    proof: &RuntimeTargetApprovalProof,
    manifest: &ProjectCapabilityManifestV1,
    target: &AppTargetV1,
    recipes: &[ExecutionRecipeV1],
    readiness: Option<&ReadinessProbeV1>,
    artifact_spec: Option<&Vec<String>>,
) -> Result<(), String> {
    receipt.validate()?;
    if receipt.receipt_uid != proof.receipt_uid()
        || target.readiness_probe_ref.is_some()
        || !target.port_needs.is_empty()
        || receipt.actor != GateActor::User
        || receipt.decision != GateDecision::Approved
        || receipt.subject_uid != target.owning_task_id
        || !receipt
            .evidence_refs
            .iter()
            .any(|item| item == &format!("runtime-target:{}", target.target_id))
        || receipt.subject_sha256
            != proposal_sha256(
                &manifest.workspace_id,
                &manifest.repository_id,
                &manifest.source_sha,
                target,
                recipes,
                readiness,
                artifact_spec,
            )?
    {
        return Err("runtime target approval receipt does not match the proposed target".into());
    }
    Ok(())
}

pub(super) fn approval_path(uid: &str) -> String {
    format!("{ROOT}/{uid}.json")
}

pub(crate) fn proposal_sha256(
    workspace_id: &str,
    repository_id: &str,
    source_sha: &str,
    target: &AppTargetV1,
    recipes: &[ExecutionRecipeV1],
    readiness: Option<&ReadinessProbeV1>,
    artifact_spec: Option<&Vec<String>>,
) -> Result<String, String> {
    let mut normalized_target = target.clone();
    // These are generated by persistence from the separately bound probe.
    normalized_target.readiness_probe_ref = None;
    normalized_target.port_needs.clear();
    let payload = serde_json::to_vec(&(
        workspace_id,
        repository_id,
        source_sha,
        normalized_target,
        recipes,
        readiness,
        artifact_spec,
    ))
    .map_err(|error| format!("runtime target approval payload is invalid: {error}"))?;
    use sha2::Digest;
    Ok(format!("{:x}", sha2::Sha256::digest(payload)))
}

/// Fingerprint the immutable shape of a custom target while ignoring the
/// shared fields collaborators can relabel to disguise its origin.
pub(crate) fn custom_target_origin_sha256(
    target: &AppTargetV1,
    recipes: &[ExecutionRecipeV1],
    readiness: Option<&ReadinessProbeV1>,
    artifact_spec: Option<&Vec<String>>,
) -> Result<String, String> {
    let mut target = target.clone();
    target.target_id.clear();
    target.readiness_probe_ref = None;
    target.port_needs.clear();
    let mut recipes = recipes.to_vec();
    for recipe in &mut recipes {
        recipe.source = crate::project_runtime::RecipeSource::TrustedAdapter;
        recipe
            .evidence_refs
            .retain(|reference| !reference.starts_with("user-approval:"));
    }
    let payload = serde_json::to_vec(&(target, recipes, readiness, artifact_spec))
        .map_err(|error| format!("custom target origin payload is invalid: {error}"))?;
    use sha2::Digest;
    Ok(format!("{:x}", sha2::Sha256::digest(payload)))
}

pub(crate) fn discovered_target_origin_sha256(
    workspace_id: &str,
    source_sha: &str,
    target: &AppTargetV1,
) -> Result<String, String> {
    let mut target = target.clone();
    target.build_recipe_ref = None;
    target.test_recipe_refs.clear();
    target.start_recipe_ref = None;
    target.readiness_probe_ref = None;
    target.port_needs.clear();
    digest_payload(&(workspace_id, source_sha, target))
}

pub(crate) fn trusted_adapter_recipe_sha256(
    workspace_id: &str,
    source_sha: &str,
    target: &AppTargetV1,
    recipes: &[ExecutionRecipeV1],
    readiness: Option<&ReadinessProbeV1>,
    readiness_recipe: Option<&ExecutionRecipeV1>,
    artifact_spec: Option<&Vec<String>>,
) -> Result<String, String> {
    digest_payload(&(
        workspace_id,
        source_sha,
        target,
        recipes,
        readiness,
        readiness_recipe,
        artifact_spec,
    ))
}

pub(super) fn digest_payload(value: &impl Serialize) -> Result<String, String> {
    let payload = serde_json::to_vec(value)
        .map_err(|error| format!("runtime target provenance payload is invalid: {error}"))?;
    use sha2::Digest;
    Ok(format!("{:x}", sha2::Sha256::digest(payload)))
}
