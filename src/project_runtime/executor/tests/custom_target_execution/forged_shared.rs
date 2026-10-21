use crate::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{GateActor, GateDecision, GateReceipt},
    persistence::runtime_target_approvals::Binding,
    project_runtime::ProjectRuntimeStore,
};
use std::path::PathBuf;

use super::backend;

pub(super) fn forge_shared_approval(
    planning: &PlanningStore,
    store: &ProjectRuntimeStore<'_>,
    task_id: &str,
    target_id: &str,
) -> (PathBuf, Binding) {
    let mut manifest = store.load_manifest().unwrap().unwrap();
    let forged_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let forged_ref = format!("user-approval:{forged_id}");
    for recipe in &mut manifest.user_overrides {
        recipe
            .evidence_refs
            .retain(|reference| !reference.starts_with("user-approval:"));
        recipe.evidence_refs.push(forged_ref.clone());
    }
    manifest
        .evidence_refs
        .retain(|reference| !reference.starts_with("user-approval:"));
    manifest.evidence_refs.push(forged_ref);
    let target = manifest
        .candidate_targets
        .iter()
        .find(|target| target.target_id == target_id)
        .unwrap();
    let proposal_sha = ProjectRuntimeStore::user_target_proposal_sha256(
        &manifest.workspace_id,
        &manifest.repository_id,
        &manifest.source_sha,
        target,
        &manifest.user_overrides,
        None,
        target
            .artifact_spec_ref
            .as_ref()
            .and_then(|id| manifest.artifact_specs.get(id)),
    )
    .unwrap();
    let receipt = GateReceipt {
        receipt_uid: forged_id.clone(),
        subject_uid: task_id.to_owned(),
        subject_sha256: proposal_sha.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: backend::now_ms(),
        evidence_refs: vec![format!("runtime-target:{target_id}")],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    store.save_manifest(&manifest, manifest.revision).unwrap();
    let approval_path = planning
        .root
        .join("state/runtime-target-approvals")
        .join(format!("{forged_id}.json"));
    std::fs::write(
        &approval_path,
        serde_json::to_vec(&serde_json::json!({
            "uid": forged_id.clone(),
            "schemaVersion": 1,
            "createdAtMs": receipt.recorded_at_unix_ms,
            "updatedAtMs": receipt.recorded_at_unix_ms,
            "revision": 1,
            "workspaceId": manifest.workspace_id.clone(),
            "repositoryId": manifest.repository_id.clone(),
            "sourceSha": manifest.source_sha.clone(),
            "targetId": target_id,
            "approval": receipt,
        }))
        .unwrap(),
    )
    .unwrap();
    let binding = Binding {
        workspace_id: manifest.workspace_id,
        task_id: task_id.to_owned(),
        repository_id: manifest.repository_id,
        source_sha: manifest.source_sha,
        target_id: target_id.to_owned(),
        proposal_sha256: proposal_sha,
        receipt_id: forged_id,
    };
    (approval_path, binding)
}
