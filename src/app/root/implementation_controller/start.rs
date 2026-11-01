use super::*;

mod actions;
mod claim;
mod identity;
mod implementation;
mod options;
mod preflight;
mod publication_retry;
mod queue_wait;
mod review;
mod self_improvement;
mod target_error;

use options::StartOptions;

#[cfg(test)]
pub(super) fn authorize_self_improvement_task_start_for_test(
    planning: &crate::artifacts::planning_store::PlanningStore,
    documents: &[crate::artifacts::task_docs::TaskDocument],
    ticket: &str,
    code_repository_id: &str,
) -> Result<bool, String> {
    self_improvement::authorize_task_start(planning, documents, ticket, code_repository_id)
}
