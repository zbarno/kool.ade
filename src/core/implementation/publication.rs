use super::*;
mod payload;
mod pull_request;
#[cfg(test)]
pub(super) use payload::remote_repository;
pub(super) use payload::{
    effective_push_remote, pr_body, push_identity_remote, validate_target_remotes,
};
pub(super) use pull_request::{create_pull_request, fenced_push};

const HOLD_FOR_REVIEW_NOTE: &str =
    "Verified locally. No remote changes were made; share for review when ready.";
const PR_STATUS_PREFIX: &str = "Pull request status: ";

pub(super) fn auto_publish_enabled(gate: Option<&AtomicBool>) -> bool {
    gate.is_none_or(|gate| gate.load(Ordering::SeqCst))
}

pub(super) fn hold_for_review(dir: &Path, state: &mut Implementation) -> anyhow::Result<()> {
    state.auto_merge = false;
    state.merged_commit = None;
    state.status = ImplementationStatus::AwaitingApproval;
    update_pull_request_detail(state);
    if !state
        .detail
        .lines()
        .any(|line| line == HOLD_FOR_REVIEW_NOTE)
    {
        if !state.detail.is_empty() {
            state.detail.push('\n');
        }
        state.detail.push_str(HOLD_FOR_REVIEW_NOTE);
    }
    save(dir, state)
}

pub(super) fn update_pull_request_detail(state: &mut Implementation) {
    state.detail = state
        .detail
        .lines()
        .filter(|line| *line != HOLD_FOR_REVIEW_NOTE && !line.starts_with(PR_STATUS_PREFIX))
        .collect::<Vec<_>>()
        .join("\n");
    let (Some(status), Some(url)) = (state.pr_state, state.pr_url.as_deref()) else {
        return;
    };
    if !state.detail.is_empty() {
        state.detail.push('\n');
    }
    let status = match status {
        PullRequestState::Open => format!("Open: {url}"),
        PullRequestState::Closed => format!("Closed: {url}"),
        PullRequestState::Merged => {
            let commit = state
                .merged_commit
                .as_deref()
                .and_then(|commit| commit.get(..12))
                .map(|commit| format!(" as {commit}"))
                .unwrap_or_default();
            format!("Merged{commit}: {url}")
        }
    };
    state.detail.push_str(PR_STATUS_PREFIX);
    state.detail.push_str(&status);
}

pub(super) fn default_branch(repo: &Path, runner: &Runner) -> anyhow::Result<String> {
    let refs = runner.git(repo, &["ls-remote", "--symref", "origin", "HEAD"])?;
    if let Some(branch) = refs.lines().find_map(|line| {
        line.strip_prefix("ref: refs/heads/")
            .and_then(|rest| rest.split_whitespace().next())
    }) {
        return Ok(branch.into());
    }
    let branch = runner.git(repo, &["symbolic-ref", "--short", "HEAD"])?;
    anyhow::ensure!(
        matches!(branch.as_str(), "main" | "master"),
        "Origin has no default branch; configure its HEAD before Auto mode"
    );
    Ok(branch)
}

pub(super) fn finish_auto_publish(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
) -> anyhow::Result<()> {
    state.status = ImplementationStatus::Completed;
    save(dir, state)?;
    if state.task_repository_kind == TaskRepositoryKind::Clone {
        return Ok(());
    }
    // Remote publication is already durable. Update an idle clean checkout only;
    // a dirty or divergent checkout remains untouched and does not undo success.
    // The writer gate keeps this fast-forward from trampling an in-flight
    // planning checkpoint (turns may commit while workers publish).
    let _guard = crate::core::writer_gate::acquire();
    if runner
        .git(repo, &["status", "--porcelain"])
        .is_ok_and(|status| status.is_empty())
        && runner
            .git(repo, &["symbolic-ref", "--short", "HEAD"])
            .is_ok_and(|branch| branch == state.base)
        && let Some(commit) = &state.merged_commit
        && let Err(error) = runner.git(repo, &["merge", "--ff-only", commit])
    {
        runner.update(format!(
            "Task merged remotely; local checkout could not fast-forward: {error}"
        ));
    }
    Ok(())
}
