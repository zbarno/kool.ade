use super::*;

pub(in crate::core::implementation::execution) fn missing_saved_ref(
    repo: &Path,
    state: &Implementation,
    cache: &RepositoryCache,
    runner: &Runner,
) -> anyhow::Result<Option<String>> {
    let repository_id = state.repository_id.as_deref().unwrap_or("unknown");
    let Some(source_branch) = state.source_ref.as_deref() else {
        return Ok(None);
    };
    if !branch_exists(repo, source_branch, runner)?
        && !remote_branch_exists(cache, source_branch, runner)?
    {
        return Ok(Some(format!(
            "Selected source branch '{source_branch}' in code repository '{repository_id}' no longer exists. The task clone was preserved; restore the ref before resuming."
        )));
    }
    let destination_branch = state
        .destination_branch
        .as_deref()
        .unwrap_or(state.base.as_str());
    let destination_exists = if cache.fetch_url.is_some() {
        remote_branch_exists(cache, destination_branch, runner)?
    } else {
        branch_exists(repo, destination_branch, runner)?
    };
    if !destination_exists {
        return Ok(Some(format!(
            "Selected destination branch '{destination_branch}' in code repository '{repository_id}' no longer exists. The task clone was preserved; restore the ref or select an existing destination before resuming."
        )));
    }
    Ok(None)
}

fn branch_exists(repo: &Path, branch: &str, runner: &Runner) -> anyhow::Result<bool> {
    runner.git(repo, &["check-ref-format", "--branch", branch])?;
    Ok(runner
        .git(
            repo,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
        )
        .is_ok())
}

fn remote_branch_exists(
    cache: &RepositoryCache,
    branch: &str,
    runner: &Runner,
) -> anyhow::Result<bool> {
    let Some(remote) = cache.fetch_url.as_deref() else {
        return Ok(false);
    };
    runner.git(&cache.path, &["check-ref-format", "--branch", branch])?;
    let result = runner.git(
        &cache.path,
        &[
            "ls-remote",
            "--heads",
            remote,
            &format!("refs/heads/{branch}"),
        ],
    )?;
    Ok(!result.is_empty())
}
