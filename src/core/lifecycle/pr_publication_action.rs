use super::PullRequestPreparationStatus as PrStatus;
use crate::core::implementation::ImplementationStatus;

pub fn pull_request_publication_action_enabled(
    lifecycle_status: Option<PrStatus>,
    implementation_status: ImplementationStatus,
    has_pull_request_url: bool,
) -> bool {
    match lifecycle_status {
        Some(PrStatus::AwaitingAuthorization) => {
            matches!(
                implementation_status,
                ImplementationStatus::ReadyToPublish | ImplementationStatus::AwaitingApproval
            ) && !has_pull_request_url
        }
        Some(PrStatus::NeedsAttention) => {
            implementation_status == ImplementationStatus::Completed && !has_pull_request_url
        }
        _ => false,
    }
}

pub fn pull_request_gate_refresh_action_enabled(
    task_phase: Option<super::TaskPhase>,
    implementation_status: ImplementationStatus,
    has_pull_request_url: bool,
) -> bool {
    task_phase == Some(super::TaskPhase::ReReview)
        && implementation_status == ImplementationStatus::AwaitingApproval
        && !has_pull_request_url
}

#[cfg(test)]
mod tests {
    use super::pull_request_publication_action_enabled as enabled;
    use crate::core::{
        implementation::ImplementationStatus as Implementation,
        lifecycle::PullRequestPreparationStatus as Pr,
    };

    #[test]
    fn initial_publication_requires_reviewed_unpublished_implementation() {
        assert!(enabled(
            Some(Pr::AwaitingAuthorization),
            Implementation::ReadyToPublish,
            false
        ));
        assert!(enabled(
            Some(Pr::AwaitingAuthorization),
            Implementation::AwaitingApproval,
            false
        ));
        assert!(!enabled(
            Some(Pr::AwaitingAuthorization),
            Implementation::Completed,
            false
        ));
        assert!(!enabled(
            Some(Pr::AwaitingAuthorization),
            Implementation::ReadyToPublish,
            true
        ));
    }

    #[test]
    fn retry_requires_completed_needs_attention_work_without_known_pr_url() {
        assert!(enabled(
            Some(Pr::NeedsAttention),
            Implementation::Completed,
            false
        ));
        assert!(!enabled(
            Some(Pr::NeedsAttention),
            Implementation::Completed,
            true
        ));
        assert!(!enabled(
            Some(Pr::NeedsAttention),
            Implementation::ReadyToPublish,
            false
        ));
        assert!(!enabled(Some(Pr::Opened), Implementation::Completed, false));
    }

    #[test]
    fn gate_refresh_requires_re_review_and_no_published_pr() {
        use super::pull_request_gate_refresh_action_enabled as refresh;
        use crate::core::lifecycle::TaskPhase;

        assert!(refresh(
            Some(TaskPhase::ReReview),
            Implementation::AwaitingApproval,
            false
        ));
        assert!(!refresh(
            Some(TaskPhase::Done),
            Implementation::AwaitingApproval,
            false
        ));
        assert!(!refresh(
            Some(TaskPhase::ReReview),
            Implementation::Completed,
            false
        ));
        assert!(!refresh(
            Some(TaskPhase::ReReview),
            Implementation::AwaitingApproval,
            true
        ));
    }
}
