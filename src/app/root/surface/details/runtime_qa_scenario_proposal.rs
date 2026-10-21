use crate::{app::session::Project, ui::task_detail::RuntimeQaScenarioProposal};

pub(super) fn for_ticket(project: &Project, ticket: &str) -> Option<RuntimeQaScenarioProposal> {
    let item_uid = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)?
        .identity
        .as_ref()?
        .uid
        .as_str();
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
    let task = lifecycle.task_for_planning_item(item_uid).ok()??;
    if task.phase != crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa {
        return None;
    }
    let request = task.qa_execution.as_ref()?.request.as_ref()?;
    let store = crate::runtime_qa::RuntimeQaStore::new(
        crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
            project.state.planning_store.project_id,
        )
        .ok()?,
    );
    let proposal = store.load_scenario_proposal(request).ok();
    let stale = proposal
        .as_ref()
        .is_some_and(|proposal| proposal.task_revision != task.revision);
    let approved = store
        .load_approved_scenario_proposal(request, task.revision)
        .is_ok();
    Some(RuntimeQaScenarioProposal {
        task_revision: task.revision,
        proposal_sha256: proposal
            .as_ref()
            .and_then(|proposal| proposal.proposal_sha256().ok()),
        scenarios: proposal.map_or_else(Vec::new, |proposal| proposal.scenarios),
        approved,
        stale,
    })
}
