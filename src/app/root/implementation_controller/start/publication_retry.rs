use crate::core::implementation::ImplementationStatus;

pub(super) fn is_terminal(
    status: ImplementationStatus,
    has_pull_request_url: bool,
    lifecycle_publication: bool,
) -> bool {
    has_pull_request_url || (status == ImplementationStatus::Completed && !lifecycle_publication)
}

#[cfg(test)]
mod tests {
    use super::is_terminal;
    use crate::core::implementation::ImplementationStatus as Status;

    #[test]
    fn completed_work_only_restarts_for_an_explicit_lifecycle_pr_action() {
        assert!(is_terminal(Status::Completed, false, false));
        assert!(!is_terminal(Status::Completed, false, true));
        assert!(is_terminal(Status::Completed, true, true));
        assert!(!is_terminal(Status::ReadyToPublish, false, false));
    }
}
