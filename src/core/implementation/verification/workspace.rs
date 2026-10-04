use super::*;

pub(super) fn prepare_worktree(
    repo: &Path,
    state: &Implementation,
    runner: &Runner,
) -> anyhow::Result<(bool, String)> {
    runner.update("Preparing implementation worktree…");
    if state.worktree.exists() {
        anyhow::ensure!(
            common(&state.worktree)?.canonicalize()? == common(repo)?.canonicalize()?,
            "Existing worktree belongs to a different repository; no changes made"
        );
        anyhow::ensure!(
            runner.git(&state.worktree, &["symbolic-ref", "--short", "HEAD"])? == state.branch,
            "Existing worktree is on another branch; no changes made"
        );
    } else {
        let path = state
            .worktree
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 worktree path"))?;
        let branch_exists = runner
            .git(
                repo,
                &[
                    "show-ref",
                    "--verify",
                    &format!("refs/heads/{}", state.branch),
                ],
            )
            .is_ok();
        if branch_exists {
            runner.git(repo, &["worktree", "add", path, &state.branch])?;
        } else {
            runner.git(
                repo,
                &[
                    "worktree",
                    "add",
                    "-b",
                    &state.branch,
                    path,
                    &state.base_commit,
                ],
            )?;
        }
    }
    let clean = runner
        .git(&state.worktree, &["status", "--porcelain"])?
        .is_empty();
    let head = runner.git(&state.worktree, &["rev-parse", "HEAD"])?;
    Ok((clean, head))
}
