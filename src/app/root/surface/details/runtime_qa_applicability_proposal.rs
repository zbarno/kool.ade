use crate::{app::session::Project, core::lifecycle::LifecycleStore};

pub(super) fn for_ticket(
    project: &Project,
    ticket: &str,
) -> Result<Option<crate::ui::task_detail::RuntimeQaApplicabilityProposal>, String> {
    let Some(document) = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
    else {
        return Ok(None);
    };
    let Some(item_uid) = document
        .metadata
        .as_ref()
        .map(|metadata| metadata.uid.as_str())
    else {
        return Ok(None);
    };
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let Some(task) = lifecycle
        .task_for_planning_item(item_uid)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    if task.phase != crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa {
        return Ok(None);
    }
    let (Some(code_sha), Some(spec_hash)) = (
        task.reviewed_code_head_sha.as_deref(),
        Some(task.approved_plan_sha256.as_str()),
    ) else {
        return Ok(None);
    };
    let store = crate::runtime_qa::RuntimeQaStore::new(
        crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
            project.state.planning_store.project_id,
        )
        .map_err(|error| error.to_string())?,
    );
    let proposal = store.latest_applicability_proposal(&task.task_uid, code_sha, spec_hash)?;
    let Some(proposal) = proposal else {
        return Ok(None);
    };
    let manifest = crate::project_runtime::ProjectRuntimeStore::new(&project.state.planning_store)
        .load_manifest()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project capability manifest is unavailable".to_owned())?;
    let active_target = manifest.candidate_targets.iter().find(|target| {
        target.target_id == proposal.assessment.target_id
            && target.app_surface == crate::project_runtime::AppSurface::HeadlessService
            && target.repository_id == manifest.repository_id
            && manifest.is_active_target(item_uid, &target.target_id)
    });
    let target_is_active = active_target.is_some();
    let manifest_hash = {
        use sha2::Digest as _;
        format!(
            "{:x}",
            sha2::Sha256::digest(serde_json::to_vec(&manifest).map_err(|error| error.to_string())?)
        )
    };
    let receipt_refs_match = active_target.is_some_and(|target| {
        let expected =
            crate::project_runtime::ProjectRuntimeStore::new(&project.state.planning_store)
                .headless_service_test_receipts(item_uid, target, &manifest.source_sha)
                .ok()
                .map(|receipts| {
                    receipts
                        .iter()
                        .map(|receipt| format!("runtime-receipt:{}", receipt.receipt_id))
                        .collect::<std::collections::BTreeSet<_>>()
                });
        let recorded = proposal
            .assessment
            .evidence_refs
            .iter()
            .filter(|reference| reference.starts_with("runtime-receipt:"))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        expected.is_some_and(|expected| expected == recorded)
    });
    let manifest_matches = manifest.source_sha == proposal.assessment.code_sha
        && task.reviewed_code_head_sha.as_deref() == Some(&manifest.source_sha)
        && document
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.repository_id == manifest.repository_id)
        && project
            .implementation_states
            .get(ticket)
            .is_some_and(|implementation| {
                implementation.verified_head.as_deref() == Some(&manifest.source_sha)
                    && implementation.workspace_uid.as_deref() == Some(&manifest.workspace_id)
            })
        && proposal
            .assessment
            .evidence_refs
            .iter()
            .any(|reference| reference == &format!("runtime-manifest:{manifest_hash}"));
    Ok(Some(
        crate::ui::task_detail::RuntimeQaApplicabilityProposal {
            proposal_sha256: proposal.proposal_sha256()?,
            stale: proposal.task_revision != task.revision
                || !target_is_active
                || !manifest_matches
                || !receipt_refs_match,
            proposal,
        },
    ))
}
