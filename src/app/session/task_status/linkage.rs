use super::shared_status;
use crate::core::implementation::{Implementation, ImplementationStatus, PullRequestState};
use crate::core::planning_work::WorkStatus;

pub(super) fn task_record_matches_previous(
    task_state: Option<&crate::artifacts::task_docs::TaskState>,
    previous: &Implementation,
    metadata: &crate::artifacts::task_docs::TaskMetadata,
) -> bool {
    let Some(task_state) = task_state else {
        return true;
    };
    let (previous_status, previous_execution_status) = shared_status(previous);
    let linkage = execution_linkage(metadata, previous);
    (task_state.status == previous_status
        && task_state.execution_status.as_deref() == Some(previous_execution_status)
        && (task_state.repository_id == linkage.repository_id
            && task_state.source_branch == linkage.source_branch
            && task_state.source_sha == linkage.source_sha
            && task_state.destination_branch == linkage.destination_branch
            && task_state.implementation_branch == linkage.implementation_branch
            && task_state.publication_status == linkage.publication_status
            && task_state.pull_requests == linkage.pull_requests
            || task_state.legacy_linkage))
        || (task_state.status == WorkStatus::Todo
            && task_state.execution_status.is_none()
            && task_state.revision == 1)
}

pub(super) fn execution_linkage(
    metadata: &crate::artifacts::task_docs::TaskMetadata,
    state: &Implementation,
) -> crate::artifacts::task_docs::TaskExecutionLinkage {
    use crate::artifacts::task_docs::{
        ExecutionPublicationStatus as LinkStatus, PullRequestLinkage, TaskExecutionLinkage,
    };

    let publication_status = match state.pr_state {
        Some(PullRequestState::Open) => LinkStatus::Shared,
        Some(PullRequestState::Closed) => LinkStatus::ClosedUnmerged,
        Some(PullRequestState::Merged) => LinkStatus::Merged,
        None if state.merged_commit.is_some() => LinkStatus::Merged,
        None => match state.status {
            ImplementationStatus::Preparing
            | ImplementationStatus::Implementing
            | ImplementationStatus::Verifying
            | ImplementationStatus::Publishing
            | ImplementationStatus::WaitingForIndependentChecks
            | ImplementationStatus::WaitingToMerge => LinkStatus::Running,
            ImplementationStatus::ReadyToPublish
            | ImplementationStatus::AwaitingApproval
            | ImplementationStatus::ChangesRequested
            | ImplementationStatus::AwaitingReview => LinkStatus::Verified,
            ImplementationStatus::Completed => LinkStatus::Verified,
            ImplementationStatus::PullRequestClosed => LinkStatus::ClosedUnmerged,
            ImplementationStatus::Blocked | ImplementationStatus::Interrupted => LinkStatus::Failed,
        },
    };
    let repository_id = state
        .repository_id
        .clone()
        .unwrap_or_else(|| metadata.repository_id.clone());
    let pull_requests = state
        .pr_url
        .as_ref()
        .and_then(|url| {
            let details = state.pr_details.as_ref();
            let number = details
                .map(|details| details.number)
                .or_else(|| pull_request_number(url))?;
            let head_sha = details
                .map(|details| details.head_sha.as_str())
                .or(state.verified_head.as_deref())?;
            let base_branch = details
                .map(|details| details.base_branch.clone())
                .or_else(|| state.destination_branch.clone())
                .unwrap_or_else(|| state.base.clone());
            Some(PullRequestLinkage {
                repository_id: repository_id.clone(),
                number,
                url: url.clone(),
                head_sha: head_sha.to_owned(),
                base_branch,
                merge_sha: state.merged_commit.clone(),
                status: publication_status,
            })
        })
        .filter(|pull_request| {
            pull_request.number > 0
                && matches!(pull_request.head_sha.len(), 40 | 64)
                && pull_request
                    .head_sha
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        })
        .into_iter()
        .collect();

    TaskExecutionLinkage {
        repository_id,
        source_branch: state
            .source_ref
            .clone()
            .or_else(|| state.source_branch.clone())
            .or_else(|| metadata.source_branch.clone()),
        source_sha: state.source_commit.clone(),
        destination_branch: state
            .destination_branch
            .clone()
            .or_else(|| metadata.destination_branch.clone())
            .or_else(|| Some(state.base.clone())),
        implementation_branch: (!state.branch.is_empty()).then(|| state.branch.clone()),
        publication_status,
        pull_requests,
    }
}

fn pull_request_number(url: &str) -> Option<u64> {
    let (_, path) = url.strip_prefix("https://")?.split_once('/')?;
    let mut segments = path.trim_end_matches('/').rsplit('/');
    let number = segments.next()?.parse::<u64>().ok()?;
    if segments.next()? != "pull" {
        return None;
    }
    (number > 0).then_some(number)
}
