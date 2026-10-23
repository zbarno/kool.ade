use crate::core::{
    lifecycle::PullRequestPreparationStatus as Status,
    self_improvement::SelfImprovementParentTaskProgressV1,
};

mod ancestry;
use ancestry::parent_merge_ancestry_verified as verify_parent_ancestry;
#[cfg(test)]
use ancestry::{
    ancestry_compare_with_timeout as compare_parent_ancestry,
    verify_parent_merge_ancestry as verify_parent_ancestry_sync,
};

pub(super) fn parent_merge_ancestry_verified(
    activity: &mut crate::app::manager::WorkspaceActivity,
    canonical_remote: &str,
    progress: &[SelfImprovementParentTaskProgressV1],
    latest_sha: &str,
    gh_bin: &std::path::Path,
) -> Result<bool, String> {
    verify_parent_ancestry(activity, canonical_remote, progress, latest_sha, gh_bin)
}

#[cfg(test)]
pub(super) fn verify_parent_merge_ancestry(
    canonical_remote: &str,
    progress: &[SelfImprovementParentTaskProgressV1],
    latest_sha: &str,
    gh_bin: &std::path::Path,
) -> bool {
    verify_parent_ancestry_sync(canonical_remote, progress, latest_sha, gh_bin)
}

#[cfg(test)]
pub(super) fn ancestry_compare_with_timeout(
    gh_bin: &std::path::Path,
    comparison: &str,
    timeout: std::time::Duration,
) -> Result<std::process::Output, String> {
    compare_parent_ancestry(gh_bin, comparison, timeout)
}

pub(super) fn closed_unmerged_parent(
    progress: &[SelfImprovementParentTaskProgressV1],
) -> Option<(String, String)> {
    if !progress.iter().all(|item| {
        matches!(
            item.pr_status,
            Some(Status::Merged | Status::ClosedUnmerged)
        )
    }) {
        return None;
    }
    progress.iter().find_map(|item| {
        (item.pr_status == Some(Status::ClosedUnmerged))
            .then(|| {
                item.pr_url
                    .as_ref()
                    .map(|url| (item.task_uid.clone(), url.clone()))
            })
            .flatten()
    })
}

pub(super) fn reopened_parent(progress: &[SelfImprovementParentTaskProgressV1]) -> Option<String> {
    if progress.is_empty()
        || !progress
            .iter()
            .all(|item| matches!(item.pr_status, Some(Status::Opened | Status::Merged)))
    {
        return None;
    }
    progress
        .iter()
        .find(|item| item.pr_status == Some(Status::Opened))
        .map(|item| item.task_uid.clone())
}

pub(super) fn aggregate_merged_parent_sha(
    progress: &[SelfImprovementParentTaskProgressV1],
    destination_ref: Option<&str>,
) -> Option<String> {
    let destination = destination_ref?
        .strip_prefix("refs/heads/")
        .unwrap_or(destination_ref?);
    if destination.is_empty() || progress.is_empty() {
        return None;
    }
    let mut merges = Vec::with_capacity(progress.len());
    for parent in progress {
        if parent.pr_status != Some(Status::Merged)
            || parent.pr_base_branch.as_deref() != Some(destination)
        {
            return None;
        }
        let sha = parent.pr_merged_commit_sha.as_deref()?;
        let merged_at =
            chrono::DateTime::parse_from_rfc3339(parent.pr_merged_at.as_deref()?).ok()?;
        if !(sha.len() == 40 || sha.len() == 64)
            || !sha.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        merges.push((merged_at, sha));
    }
    merges.sort_by_key(|(merged_at, _)| *merged_at);
    let latest = merges.last()?;
    if merges.len() > 1
        && merges[merges.len() - 2].0 == latest.0
        && merges[merges.len() - 2].1 != latest.1
    {
        return None;
    }
    Some(latest.1.to_owned())
}

#[cfg(test)]
#[path = "pr_evidence/tests.rs"]
mod tests;
