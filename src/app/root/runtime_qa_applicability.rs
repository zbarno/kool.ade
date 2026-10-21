use super::{KooladeApp, Screen};

mod approval;
mod review;

impl KooladeApp {
    pub(super) fn review_runtime_qa_applicability(&mut self, ticket: &str) {
        review::start(self, ticket);
    }

    pub(super) fn approve_runtime_qa_applicability(
        &mut self,
        ticket: &str,
        expected_revision: u64,
        proposal_sha256: &str,
    ) {
        let result = match &self.screen {
            Screen::Connected(project) => {
                approval::record(project, ticket, expected_revision, proposal_sha256)
            }
            Screen::Welcome => Err("Connect a project before recording QA applicability.".into()),
        };
        match result {
            Ok(()) => self.toasts.success(
                "Recorded that visual QA does not apply to this headless target. Service and API tests remain separate.",
            ),
            Err(error) => self.toasts.warning(error),
        }
    }
}

pub(crate) fn persist_review(
    project: &crate::app::session::Project,
    proposal: &crate::runtime_qa::QaApplicabilityProposalV1,
) -> Result<(), String> {
    proposal.validate()?;
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
    let task = lifecycle
        .load_task(&proposal.assessment.task_uid)
        .map_err(|error| error.to_string())?;
    if task.revision != proposal.task_revision
        || task.phase != crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa
        || task.reviewed_code_head_sha.as_deref() != Some(&proposal.assessment.code_sha)
        || task.approved_plan_sha256 != proposal.assessment.approved_spec_hash
    {
        return Err(
            "task revision or approved QA subject changed during applicability review".into(),
        );
    }
    let manifest = crate::project_runtime::ProjectRuntimeStore::new(&project.state.planning_store)
        .load_manifest()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project capability manifest is unavailable".to_owned())?;
    let planning_uid = task.planning_item_uid.as_deref().unwrap_or(&task.task_uid);
    let target = manifest
        .candidate_targets
        .iter()
        .find(|target| target.target_id == proposal.assessment.target_id)
        .ok_or_else(|| "reviewed headless target is no longer present".to_owned())?;
    if !manifest.is_active_target(planning_uid, &proposal.assessment.target_id)
        || target.app_surface != crate::project_runtime::AppSurface::HeadlessService
    {
        return Err("reviewed headless target is no longer the active target for this task".into());
    }
    let store = crate::runtime_qa::RuntimeQaStore::new(
        crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
            project.state.planning_store.project_id,
        )?,
    );
    store.save_applicability_proposal(proposal)?;
    Ok(())
}
