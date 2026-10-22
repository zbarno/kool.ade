use crate::core::implementation::ImplementationStatus;
use crate::core::implementation::PublicationMode;

pub(super) fn publication_mode(
    lifecycle_gate_refresh: bool,
    is_subtask: bool,
    allow_offline: bool,
    explicit_publish: bool,
    requested_changes: bool,
    auto_publish: bool,
) -> PublicationMode {
    if lifecycle_gate_refresh || is_subtask || allow_offline {
        PublicationMode::HoldForReview
    } else if explicit_publish {
        PublicationMode::CreatePullRequest
    } else if requested_changes {
        PublicationMode::HoldForReview
    } else if auto_publish {
        PublicationMode::AutoPublish
    } else {
        PublicationMode::HoldForReview
    }
}

pub(super) fn is_terminal(
    status: ImplementationStatus,
    has_pull_request_url: bool,
    lifecycle_publication: bool,
    lifecycle_gate_refresh: bool,
) -> bool {
    has_pull_request_url
        || (status == ImplementationStatus::Completed
            && !lifecycle_publication
            && !lifecycle_gate_refresh)
}

#[cfg(test)]
mod tests {
    use super::is_terminal;
    use crate::core::implementation::ImplementationStatus as Status;

    #[test]
    fn completed_work_only_restarts_for_an_explicit_lifecycle_pr_action() {
        assert!(is_terminal(Status::Completed, false, false, false));
        assert!(!is_terminal(Status::Completed, false, true, false));
        assert!(!is_terminal(Status::Completed, false, false, true));
        assert!(is_terminal(Status::Completed, true, true, true));
        assert!(!is_terminal(Status::ReadyToPublish, false, false, false));
    }
}
