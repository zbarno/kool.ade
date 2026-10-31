use std::path::Path;

use crate::core::managed_workspace::{ManagedWorkspace, WorkspaceManager};

use super::same_github_remote;

pub(super) fn resume_interrupted_planning_push(
    manager: &WorkspaceManager,
    code_remote: &str,
    planning_remote: &str,
) -> anyhow::Result<Option<ManagedWorkspace>> {
    let mut candidate = None;
    for local in manager.list_launcher()?.available {
        let workspace = manager.open(local.id)?;
        if !workspace
            .repositories
            .repositories
            .iter()
            .any(|repository| same_github_remote(&repository.remote, code_remote))
        {
            continue;
        }
        anyhow::ensure!(
            candidate.is_none(),
            "More than one managed workspace registers this code repository; open the existing paired workspace and retry there."
        );
        let origin = crate::core::gitops::origin_url(&workspace.planning_repo);
        anyhow::ensure!(
            origin
                .as_deref()
                .is_none_or(|origin| same_github_remote(origin, planning_remote)),
            "A managed planning checkout for this code repository has a different remote. Kool.ad/e left it untouched."
        );
        candidate = Some(workspace);
    }
    let Some(workspace) = candidate else {
        return Ok(None);
    };

    let root = &workspace.planning_repo;
    anyhow::ensure!(
        crate::core::gitops::is_work_tree(root),
        "The saved planning checkout is not a Git working tree. Kool.ad/e left it untouched."
    );
    let status = git_output(root, &["status", "--porcelain", "--untracked-files=all"])?;
    anyhow::ensure!(
        status.stdout.is_empty(),
        "The saved planning checkout has uncommitted changes. Kool.ad/e left both repositories untouched."
    );
    let workspace = manager.set_planning_remote(workspace.id, planning_remote)?;
    push_saved_branch(root, planning_remote)?;
    Ok(Some(manager.open(workspace.id)?))
}

pub(super) fn ensure_remote_empty(root: &Path, remote: &str) -> anyhow::Result<()> {
    let remote_refs = git_output(root, &["ls-remote", "--refs", "--", remote])?;
    anyhow::ensure!(
        remote_refs.stdout.is_empty(),
        "The existing planning repository already has Git refs. Kool.ad/e will not replace or merge its contents during setup retry."
    );
    Ok(())
}

#[derive(Debug)]
pub(super) struct CreateOnlyPush {
    commit: String,
    reference: String,
    remote: String,
}

pub(super) fn push_saved_branch(root: &Path, remote: &str) -> anyhow::Result<()> {
    let prepared = prepare_saved_branch_push(root, remote)?;
    publish_prepared_branch(root, &prepared)
}

pub(super) fn prepare_saved_branch_push(
    root: &Path,
    remote: &str,
) -> anyhow::Result<CreateOnlyPush> {
    let origin = crate::core::gitops::origin_url(root)
        .ok_or_else(|| anyhow::anyhow!("The saved planning checkout has no origin remote."))?;
    anyhow::ensure!(
        origin == remote || same_github_remote(&origin, remote),
        "The saved planning checkout origin does not match the requested planning repository. Kool.ad/e left it untouched."
    );
    let branch = String::from_utf8_lossy(
        &git_output(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])?.stdout,
    )
    .trim()
    .to_owned();
    anyhow::ensure!(
        !branch.is_empty(),
        "The saved planning checkout has no current branch. Kool.ad/e left the remote untouched."
    );
    let reference = format!("refs/heads/{branch}");
    let valid_ref = git_output(root, &["check-ref-format", &reference])?;
    anyhow::ensure!(
        valid_ref.status.success(),
        "The saved planning checkout has an invalid current branch. Kool.ad/e left the remote untouched."
    );
    let commit = String::from_utf8_lossy(
        &git_output(root, &["rev-parse", "--verify", "HEAD^{commit}"])?.stdout,
    )
    .trim()
    .to_owned();
    anyhow::ensure!(
        commit.bytes().all(|byte| byte.is_ascii_hexdigit()) && matches!(commit.len(), 40 | 64),
        "The saved planning checkout has an invalid commit ID. Kool.ad/e left the remote untouched."
    );
    ensure_remote_empty(root, remote)?;
    Ok(CreateOnlyPush {
        commit,
        reference,
        remote: remote.to_owned(),
    })
}

pub(super) fn publish_prepared_branch(
    root: &Path,
    prepared: &CreateOnlyPush,
) -> anyhow::Result<()> {
    let origin = crate::core::gitops::origin_url(root)
        .ok_or_else(|| anyhow::anyhow!("The saved planning checkout has no origin remote."))?;
    anyhow::ensure!(
        origin == prepared.remote || same_github_remote(&origin, &prepared.remote),
        "The saved planning checkout origin changed after setup validation. Kool.ad/e left it untouched."
    );
    // The explicit empty expectation is a create-only lease. If another
    // actor creates this branch after ls-remote, Git rejects the push even
    // when that new commit is an ancestor of ours; no remote history can be
    // replaced by this retry path.
    let lease = format!("--force-with-lease={}:", prepared.reference);
    let refspec = format!("{}:{}", prepared.commit, prepared.reference);
    git_output(root, &["push", "--porcelain", &lease, "origin", &refspec])?;
    Ok(())
}

fn git_output(root: &Path, arguments: &[&str]) -> anyhow::Result<std::process::Output> {
    let output = crate::core::planning_sync::validated_git_command(root)?
        .args(arguments)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Git {} failed: {}",
        arguments.join(" "),
        crate::error::redact_secrets(&String::from_utf8_lossy(&output.stderr))
    );
    Ok(output)
}

#[cfg(test)]
#[path = "recovery/tests.rs"]
mod tests;
