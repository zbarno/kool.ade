use super::phase_for_pr_status;
use crate::core::lifecycle::{PullRequestPreparationStatus as PrStatus, TaskPhase};

#[test]
fn persisted_pr_status_is_projected_to_the_liveness_task_phase() {
    for (status, expected) in [
        (PrStatus::AwaitingAuthorization, TaskPhase::PrPending),
        (PrStatus::Publishing, TaskPhase::PrPending),
        (PrStatus::NeedsAttention, TaskPhase::Blocked),
        (PrStatus::ClosedUnmerged, TaskPhase::Blocked),
        (PrStatus::Opened, TaskPhase::PrOpened),
        (PrStatus::Merged, TaskPhase::PrMerged),
    ] {
        assert_eq!(phase_for_pr_status(status), expected, "{status:?}");
    }
}
