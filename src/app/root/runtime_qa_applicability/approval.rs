use crate::{
    app::session::Project,
    core::lifecycle::{GateActor, GateDecision, GateReceipt, LifecycleStore, TaskPhase},
    runtime_qa::RuntimeQaStore,
};

pub(super) fn record(
    project: &Project,
    ticket: &str,
    expected_revision: u64,
    expected_proposal_sha256: &str,
) -> Result<(), String> {
    let document = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
        .ok_or_else(|| "Task details no longer identify a saved task.".to_owned())?;
    let item_uid = document
        .metadata
        .as_ref()
        .map(|metadata| metadata.uid.as_str())
        .ok_or_else(|| "Task has no persisted runtime identity.".to_owned())?;
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let task = lifecycle
        .task_for_planning_item(item_uid)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Task lifecycle record is unavailable.".to_owned())?;
    if task.revision != expected_revision || task.phase != TaskPhase::AwaitingRuntimeQa {
        return Err("The task changed since this applicability review was shown; reload it before approving.".into());
    }
    let code_head = task
        .reviewed_code_head_sha
        .as_deref()
        .ok_or_else(|| "The task has no current reviewed code.".to_owned())?;
    let store = RuntimeQaStore::new(RuntimeQaStore::local_root_for_workspace(
        project.state.planning_store.project_id,
    )?);
    let proposal = store
        .latest_applicability_proposal(&task.task_uid, code_head, &task.approved_plan_sha256)?
        .ok_or_else(|| {
            "The independent applicability review proposal is unavailable.".to_owned()
        })?;
    let proposal_hash = proposal.proposal_sha256()?;
    if proposal_hash != expected_proposal_sha256 || proposal.task_revision != task.revision {
        return Err(
            "This applicability proposal is stale or its displayed hash does not match.".into(),
        );
    }
    let manifest = crate::project_runtime::ProjectRuntimeStore::new(&project.state.planning_store)
        .load_manifest()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project capability manifest is unavailable".to_owned())?;
    let target = manifest
        .candidate_targets
        .iter()
        .find(|target| target.target_id == proposal.assessment.target_id)
        .ok_or_else(|| "reviewed headless target is no longer present".to_owned())?;
    let implementation = project
        .implementation_states
        .get(ticket)
        .ok_or_else(|| "the verified task implementation is unavailable".to_owned())?;
    let manifest_hash = sha256(&serde_json::to_vec(&manifest).map_err(|error| error.to_string())?);
    if target.app_surface != crate::project_runtime::AppSurface::HeadlessService
        || !manifest.is_active_target(item_uid, &proposal.assessment.target_id)
        || manifest.source_sha != proposal.assessment.code_sha
        || task.reviewed_code_head_sha.as_deref() != Some(&manifest.source_sha)
        || implementation.verified_head.as_deref() != Some(&manifest.source_sha)
        || implementation.workspace_uid.as_deref() != Some(&manifest.workspace_id)
        || manifest.repository_id
            != document
                .metadata
                .as_ref()
                .map(|metadata| metadata.repository_id.as_str())
                .unwrap_or_default()
        || !proposal
            .assessment
            .evidence_refs
            .iter()
            .any(|reference| reference == &format!("runtime-manifest:{manifest_hash}"))
    {
        return Err(
            "the reviewed target manifest changed; request a fresh applicability review".into(),
        );
    }
    let code_sha = task
        .implementation_sha256
        .as_deref()
        .ok_or_else(|| "Task implementation hash is missing.".to_owned())?;
    let assessment_hash =
        sha256(&serde_json::to_vec(&proposal.assessment).map_err(|e| e.to_string())?);
    let receipt = GateReceipt {
        receipt_uid: uuid::Uuid::new_v5(
            &uuid::Uuid::parse_str(&task.task_uid).map_err(|error| error.to_string())?,
            format!(
                "runtime-qa-applicability-manager-signoff:v1:{}:{proposal_hash}",
                task.revision
            )
            .as_bytes(),
        )
        .hyphenated()
        .to_string(),
        subject_uid: task.task_uid.clone(),
        subject_sha256: code_sha.to_owned(),
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::NotApplicable,
        recorded_at_unix_ms: now_ms(),
        evidence_refs: vec![
            format!("runtime-qa-applicability:{assessment_hash}"),
            format!("gate-receipt:{}", proposal.reviewer_receipt.receipt_uid),
            format!("runtime-qa-applicability-proposal:{proposal_hash}"),
        ],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: Some(proposal.assessment.approved_by.clone()),
        author_run_id: None,
        reviewer_session_id: Some(proposal.assessment.manager_session_id.clone()),
        author_session_id: None,
    };
    let event_uid = uuid::Uuid::new_v5(
        &uuid::Uuid::parse_str(&task.task_uid).map_err(|error| error.to_string())?,
        format!(
            "runtime-qa-applicability-event:v1:{}:{proposal_hash}",
            task.revision
        )
        .as_bytes(),
    )
    .hyphenated()
    .to_string();
    lifecycle
        .record_runtime_qa_applicability(
            &task.task_uid,
            expected_revision,
            &event_uid,
            proposal,
            receipt,
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(1)
}
