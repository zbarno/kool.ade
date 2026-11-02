use crate::app::session::Project;
use crate::core::lifecycle::LifecycleStore;

pub(in crate::app::root::tick::activity::spec_reconciliation) fn record_blocker(
    project: &mut Project,
    task_uid: &str,
    expected_revision: u64,
    spec_sha256: &str,
    proposal_sha256: String,
    reason: String,
    proposal_json: Option<String>,
) {
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let event_uid = super::event_uid(
        task_uid,
        expected_revision,
        "spec-reconciliation-blocked-v1",
        &proposal_sha256,
    );
    match lifecycle.block_spec_reconciliation(
        task_uid,
        expected_revision,
        &event_uid,
        crate::core::lifecycle::SpecReconciliationBlockV1 {
            reason: reason.clone(),
            proposal_sha256,
            spec_sha256: spec_sha256.to_owned(),
            proposal_json,
        },
    ) {
        Ok(_) => {
            let notice = format!(
                "Specification reconciliation for task {task_uid} is blocked: {reason} The approved specification was not changed; resolve the decision and update the specification before retrying."
            );
            project.activity.pending.push(notice.clone());
            project.remember_chat(vec![crate::domain::chatlog::ChatMessage::new(
                crate::domain::chatlog::ChatRole::System,
                notice,
                None,
            )]);
        }
        Err(error) => project.activity.pending.push(format!(
            "Specification reconciliation blocker for task {task_uid} could not be persisted: {error}"
        )),
    }
}
