use crate::{app::session::Project, ui::task_detail::LifecycleSpecProposal};

pub(super) fn for_ticket(project: &Project, ticket: &str) -> Option<LifecycleSpecProposal> {
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
    let reason = task.spec_reconciliation_blocker.clone()?;
    let proposal_sha256 = task.spec_reconciliation_proposal_sha256.clone()?;
    let base_spec_sha256 = task.spec_reconciliation_spec_sha256.clone()?;
    let proposal = task
        .spec_reconciliation_proposal_json
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok());
    Some(LifecycleSpecProposal {
        expected_revision: task.revision,
        proposal_sha256,
        base_spec_sha256,
        reason,
        proposal,
    })
}
