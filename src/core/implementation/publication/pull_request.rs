use super::*;
use crate::core::implementation::repository_cache::RepositoryCache;

pub(in crate::core::implementation) fn create_pull_request(
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
    claim_lease: Option<&crate::core::task_claim::ClaimLeaseHandle>,
) -> anyhow::Result<()> {
    runner.remaining()?;
    let repository_cache = if state.task_repository_kind == TaskRepositoryKind::Clone {
        Some(RepositoryCache::from_saved_state(state, runner)?)
    } else {
        None
    };
    if state.destination_branch.is_some() {
        let available = if let Some(cache) = repository_cache.as_ref() {
            cache
                .refresh_branch(&state.base, runner)
                .map(|_| String::new())
        } else {
            runner.git(
                &state.task_repository,
                &[
                    "ls-remote",
                    "--exit-code",
                    "--heads",
                    "origin",
                    &format!("refs/heads/{}", state.base),
                ],
            )
        };
        available.map_err(|error| {
            crate::core::implementation::initial_reconciliation::support::user_action(
                format!("Selected destination branch '{}' no longer exists on origin. Select an existing destination branch before creating the pull request. {error}", state.base),
            )
        })?;
    }
    anyhow::ensure!(
        !matches!(state.branch.as_str(), "main" | "master"),
        "Refusing to push implementation changes from the main or master branch"
    );
    anyhow::ensure!(
        state.branch.starts_with("koolade/"),
        "Refusing to push implementation changes from a non-Kool.ad/e branch"
    );
    runner.update("Publishing the verified implementation and creating its pull request…");
    // Explicit base/head and body file avoid prompts, accidental forks and shell expansion.
    let remote = if let Some(cache) = repository_cache.as_ref() {
        cache.validate_task_remote(&state.task_repository, runner)?;
        cache
            .origin_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Saved task has no trusted Git origin"))?
            .to_owned()
    } else {
        runner.git(
            &state.task_repository,
            &["config", "--get", "remote.origin.url"],
        )?
    };
    let push_identity = if let Some(cache) = repository_cache.as_ref() {
        state
            .push_repository
            .as_deref()
            .or(cache.push_identity_url.as_deref())
            .or(cache.origin_url.as_deref())
            .ok_or_else(|| anyhow::anyhow!("Saved task has no trusted push destination"))?
            .to_owned()
    } else {
        push_identity_remote(&state.task_repository, runner)?
    };
    let effective_push = if let Some(cache) = repository_cache.as_ref() {
        cache
            .push_url
            .as_deref()
            .or(cache.origin_url.as_deref())
            .ok_or_else(|| anyhow::anyhow!("Saved task has no effective push destination"))?
            .to_owned()
    } else {
        effective_push_remote(&state.task_repository, runner)?
    };
    let repository =
        validate_target_remotes(&remote, &push_identity, &effective_push).map_err(|error| {
            crate::core::implementation::initial_reconciliation::support::user_action(
                error.to_string(),
            )
        })?;
    let prs = runner.command(
        &state.task_repository,
        &runner.gh,
        &[
            "pr",
            "list",
            "--repo",
            &repository,
            "--head",
            &state.branch,
            "--base",
            &state.base,
            "--state",
            "all",
            "--json",
            "url,state",
        ],
    )?;
    let existing: Vec<serde_json::Value> = serde_json::from_str(&prs)?;
    if let Some(pr) = existing.first() {
        anyhow::ensure!(
            pr["state"] == "OPEN",
            "The existing PR is closed or merged. Review it before publishing more changes; no duplicate PR created."
        );
    }
    let current_branch =
        runner.git(&state.task_repository, &["symbolic-ref", "--short", "HEAD"])?;
    anyhow::ensure!(
        current_branch == state.branch,
        "Implementation repository branch changed; refusing to push"
    );
    let commit = state
        .verified_head
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Verified task commit is missing; refusing to publish"))?;
    if let Some(claim_lease) = claim_lease {
        fenced_push(
            claim_lease,
            &state.task_repository,
            commit,
            &format!("refs/heads/{}", state.branch),
        )?;
    } else if let Some(cache) = repository_cache.as_ref() {
        cache.push_commit(
            &state.task_repository,
            commit,
            &format!("refs/heads/{}", state.branch),
            runner,
        )?;
    } else {
        runner.git(
            &state.task_repository,
            &["push", "--set-upstream", "origin", &state.branch],
        )?;
    }
    if let Some(pr) = existing.first() {
        anyhow::ensure!(
            pr["state"] != "CLOSED",
            "The existing PR is closed. Review it before continuing; no duplicate PR created."
        );
        state.pr_url = pr["url"].as_str().map(String::from);
    } else {
        // The candidate branch and renewed claim were pushed atomically. The
        // GitHub API cannot join that Git transaction, so recheck immediately
        // before creating the PR; takeover can still race this API call.
        verify_claim(claim_lease)?;
        let body = dir.join("pr-body.md");
        let url = runner.command(
            &state.task_repository,
            &runner.gh,
            &[
                "pr",
                "create",
                "--repo",
                &repository,
                "--head",
                &state.branch,
                "--base",
                &state.base,
                "--title",
                &title(&state.ticket_text),
                "--body-file",
                body.to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid PR body path"))?,
            ],
        )?;
        state.pr_url = Some(url);
    }
    anyhow::ensure!(
        state
            .pr_url
            .as_ref()
            .is_some_and(|url| url.starts_with("https://")),
        "GitHub did not return a PR URL"
    );
    state.status = ImplementationStatus::AwaitingReview;
    state.pr_state = Some(PullRequestState::Open);
    update_pull_request_detail(state);
    Ok(())
}

fn verify_claim(
    claim_lease: Option<&crate::core::task_claim::ClaimLeaseHandle>,
) -> anyhow::Result<()> {
    if let Some(claim_lease) = claim_lease {
        claim_lease.verify().map_err(lease_lost_error)?;
    }
    Ok(())
}

pub(in crate::core::implementation) fn fenced_push(
    claim_lease: &crate::core::task_claim::ClaimLeaseHandle,
    source: &Path,
    commit: &str,
    destination_ref: &str,
) -> anyhow::Result<()> {
    claim_lease
        .fenced_push(source, commit, destination_ref)
        .map_err(lease_lost_error)
}

fn lease_lost_error(error: crate::core::task_claim::ClaimError) -> anyhow::Error {
    anyhow::Error::new(super::status::FailureCause(super::Failure::new(
        super::FailureKind::LeaseLost,
        super::RecoveryDisposition::UserAction,
        format!(
            "The remote task claim was lost or could not be verified. No publication was made. Recover the current task claim before retrying. {error}"
        ),
    )))
}
