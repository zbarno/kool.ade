use super::*;
use crate::core::implementation::repository_cache::RepositoryCache;
mod change_set;
mod prepare;
use prepare::prepare_integration;

pub(super) fn auto_publish(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    policy: &ExecutionPolicy<'_>,
) -> anyhow::Result<()> {
    integrate(repo, dir, state, harness, runner, policy, true)
}

pub(super) fn prepare_for_pull_request(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    policy: &ExecutionPolicy<'_>,
) -> anyhow::Result<()> {
    integrate(repo, dir, state, harness, runner, policy, false)
}

fn integrate(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    policy: &ExecutionPolicy<'_>,
    publish_after_integration: bool,
) -> anyhow::Result<()> {
    // Independent checks are mandated for AutoPublish regardless of the
    // explicit operator toggle (combine mirrors the execution policy).
    let require_independent_checks = policy.require_independent_checks
        || policy.publication_mode == PublicationMode::AutoPublish;
    let repository_cache = if state.task_repository_kind == TaskRepositoryKind::Clone {
        Some(RepositoryCache::from_saved_state(state, runner)?)
    } else {
        None
    };
    let integration_git_repo = repository_cache
        .as_ref()
        .map_or(repo, |cache| cache.path.as_path());
    let publish_lock_path = match repository_cache.as_ref() {
        Some(cache) => cache.path.join("koolade-auto-publish.lock"),
        None => common(repo)?.join("koolade-auto-publish.lock"),
    };
    let publish_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(publish_lock_path)?;
    state.status = ImplementationStatus::WaitingToMerge;
    save(dir, state)?;
    runner.update("Verified; waiting for the project integration lock…");
    while publish_lock.try_lock().is_err() {
        runner.remaining()?;
        if !publication::auto_publish_enabled(policy.auto_publish_gate) {
            return publication::hold_for_review(dir, state);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut last_error = String::new();
    for _ in 0..3 {
        runner.remaining()?;
        if !publication::auto_publish_enabled(policy.auto_publish_gate) {
            return publication::hold_for_review(dir, state);
        }
        runner.update(format!(
            "Integrating and verifying against destination origin/{}…",
            state.base
        ));
        let task_key = task_repository::allocation_key(state);
        let remote_ref = format!("refs/koolade-auto-bases/{task_key}");
        let refreshed = if let Some(cache) = repository_cache.as_ref() {
            cache
                .refresh_branch(&state.base, runner)
                .and_then(|commit| {
                    runner.git(integration_git_repo, &["update-ref", &remote_ref, &commit])?;
                    Ok(commit)
                })
        } else {
            runner
                .git(
                    repo,
                    &[
                        "fetch",
                        "--no-tags",
                        "--no-write-fetch-head",
                        "origin",
                        &format!("+refs/heads/{}:{remote_ref}", state.base),
                    ],
                )
                .and_then(|_| runner.git(repo, &["rev-parse", &remote_ref]))
        };
        if let Err(error) = refreshed {
            if state.destination_branch.is_some() {
                return Err(
                    crate::core::implementation::initial_reconciliation::support::user_action(
                        format!(
                            "Selected destination branch '{}' is unavailable on origin. Refresh the repository and select an existing destination branch. {error}",
                            state.base
                        ),
                    ),
                );
            }
            last_error = error.to_string();
            continue;
        }
        let remote = runner.git(integration_git_repo, &["rev-parse", &remote_ref])?;
        // Recover a crash or lost push response without another merge or agent call.
        if let Some(commit) = state.merged_commit.clone()
            && runner
                .git(
                    integration_git_repo,
                    &["merge-base", "--is-ancestor", &commit, &remote],
                )
                .is_ok()
        {
            if require_independent_checks
                && !checks_gate::independent_check_passed(state.independent_check.as_ref(), &commit)
            {
                checks_gate::wait_for_independent_checks(
                    dir,
                    state,
                    &state.task_repository.clone(),
                    &commit,
                    runner,
                    policy.auto_publish_gate,
                    policy.claim_lease,
                )?;
            }
            return publication::finish_auto_publish(repo, dir, state, runner);
        }
        let local = if state.task_repository_kind == TaskRepositoryKind::Clone
            || state.destination_branch.is_some()
        {
            remote.clone()
        } else {
            runner.git(integration_git_repo, &["rev-parse", "HEAD"])?
        };
        // Integrate on fetched remote truth when histories diverge. The task's
        // verified branch is squash-merged below, with conflicts repaired and
        // verification rerun in isolation. Never rewrite the user's checkout.
        let integration_base = if runner
            .git(
                integration_git_repo,
                &["merge-base", "--is-ancestor", &remote, &local],
            )
            .is_ok()
        {
            local
        } else {
            remote.clone()
        };
        if state.task_repository_kind == TaskRepositoryKind::Clone {
            task_repository::validate_clone_path(state)?;
            let task_head = state
                .verified_head
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Verified task commit is missing"))?;
            RepositoryCache::from_saved_state(state, runner)?.pin_task_commit(
                &state.task_repository,
                &state.branch,
                &state.base_commit,
                task_head,
                &task_repository::allocation_key(state),
                runner,
            )?;
        }
        if let Some(task_head) = state.verified_head.as_deref() {
            let conflicts = change_set::overlap(
                integration_git_repo,
                &state.base_commit,
                task_head,
                &remote,
                &dir.join("integration-change-audit"),
                runner,
            )?;
            if !conflicts.is_empty() {
                let paths = change_set::display_paths(&conflicts);
                state.detail = format!(
                    "The task and origin/{} both changed these paths: {paths}. Kool.ad/e is checking the changes in an isolated integration clone.",
                    state.base
                );
                save(dir, state)?;
                runner.update(state.detail.clone());
            }
        }
        let integration =
            prepare_integration(repo, dir, state, &integration_base, harness, runner, policy)?;
        if !publish_after_integration {
            state.branch = integration.branch.clone();
            state.task_repository = integration.task_repository.clone();
            state.task_repository_ready = integration.task_repository_ready;
            state.task_repositories = integration.task_repositories.clone();
            state.task_repository_commits = integration.task_repository_commits.clone();
            state.base_commit = integration.base_commit.clone();
            state.verified_head = integration.verified_head.clone();
            state.merged_commit = None;
            state.status = ImplementationStatus::Verifying;
            save(dir, state)?;
            return Ok(());
        }
        state.merged_commit = integration.verified_head.clone();
        state.status = ImplementationStatus::Publishing;
        save(dir, state)?;
        if !publication::auto_publish_enabled(policy.auto_publish_gate) {
            return publication::hold_for_review(dir, state);
        }
        if require_independent_checks {
            let verified_commit = state
                .merged_commit
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Integrated commit is missing"))?;
            checks_gate::wait_for_independent_checks(
                dir,
                state,
                &integration.task_repository,
                &verified_commit,
                runner,
                policy.auto_publish_gate,
                policy.claim_lease,
            )?;
        }
        // Automatic publication stops at verified local work. The operator
        // must explicitly choose Share, which publishes only the koolade branch
        // and opens a pull request; never push directly to the default branch.
        return publication::hold_for_review(dir, state);
    }
    anyhow::bail!(
        "Auto publication could not complete after retries; verified work is preserved: {last_error}"
    )
}
