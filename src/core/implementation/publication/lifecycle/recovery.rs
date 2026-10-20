use super::*;
use crate::core::implementation::{ImplementationStatus, save};
use std::path::Path;

/// An implementation which has passed its durable gates stays Done when the
/// later PR operation fails. Retry reconciles the provider before creating a PR,
/// so a timeout after a successful remote request does not create another PR.
pub(in crate::core::implementation) fn preserve_done_after_failure(
    store: &PlanningStore,
    dir: &Path,
    implementation: &mut Implementation,
    error: &anyhow::Error,
) -> anyhow::Result<bool> {
    let Some(planning_uid) = implementation.task_uid.as_deref() else {
        return Ok(false);
    };
    let lifecycle = LifecycleStore::new(store);
    let Some(task) = lifecycle.task_for_planning_item(planning_uid)? else {
        return Ok(false);
    };
    if task.phase != crate::core::lifecycle::TaskPhase::Done {
        return Ok(false);
    }
    let work_uid = task
        .pr_work_item_uid
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Done implementation has no PR work item"))?;
    let work = lifecycle.load_pr_preparation(work_uid)?;
    let reason = crate::error::redact_secrets(&format!("{error:#}"));
    if matches!(
        work.status,
        PullRequestPreparationStatus::AwaitingAuthorization
            | PullRequestPreparationStatus::Publishing
            | PullRequestPreparationStatus::NeedsAttention
    ) {
        lifecycle.record_pr_publication_failure(
            work_uid,
            work.revision,
            &uuid::Uuid::new_v4().to_string(),
            &reason,
        )?;
        implementation.status = ImplementationStatus::Completed;
        implementation.detail = format!(
            "Implementation is Done. PR publication needs attention: {}\nRetry publication after addressing the blocker. The remote outcome may be unknown; retry checks for an existing PR before creating one.",
            crate::core::context_build::clip(&reason, 4000),
        );
    } else {
        // A late lease/cancellation error cannot undo a provider observation
        // already recorded durably. Keep its separate PR status visible.
        implementation.status = if work.status == PullRequestPreparationStatus::Merged {
            ImplementationStatus::Completed
        } else {
            ImplementationStatus::AwaitingReview
        };
        implementation.detail = format!(
            "Implementation is Done. PR work remains {:?}: {}",
            work.status,
            crate::core::context_build::clip(&reason, 4000)
        );
    }
    implementation.auto_merge = false;
    save(dir, implementation)?;
    Ok(true)
}
