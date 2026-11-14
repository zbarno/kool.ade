use crate::core::lifecycle::{
    LifecycleStore, PullRequestPreparationStatus as PrStatus, TaskPhase, TaskRecord,
};

#[cfg(test)]
#[path = "projection/tests.rs"]
mod tests;

pub(super) fn task_phases(
    lifecycle: &LifecycleStore<'_>,
    tasks: &[&TaskRecord],
) -> Result<Vec<TaskPhase>, String> {
    tasks
        .iter()
        .map(|task| {
            let Some(work_uid) = task.pr_work_item_uid.as_deref() else {
                return Ok(task.phase);
            };
            let preparation = lifecycle
                .load_pr_preparation(work_uid)
                .map_err(|error| error.to_string())?;
            if preparation.task_uid != task.task_uid || preparation.feature_uid != task.feature_uid
            {
                return Err(format!(
                    "PR work item {work_uid} does not belong to lifecycle task {}",
                    task.task_uid
                ));
            }
            Ok(phase_for_pr_status(preparation.status))
        })
        .collect()
}

fn phase_for_pr_status(status: PrStatus) -> TaskPhase {
    match status {
        PrStatus::AwaitingAuthorization | PrStatus::Publishing => TaskPhase::PrPending,
        PrStatus::NeedsAttention | PrStatus::ClosedUnmerged => TaskPhase::Blocked,
        PrStatus::Opened => TaskPhase::PrOpened,
        PrStatus::Merged => TaskPhase::PrMerged,
    }
}
