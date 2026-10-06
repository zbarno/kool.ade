use super::*;
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
    let publish_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(common(repo)?.join("koolade-auto-publish.lock"))?;
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
        let remote_ref = format!("refs/koolade-auto-bases/{}", key(&state.ticket));
        if let Err(error) = runner.git(
            repo,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                "origin",
                &format!("+refs/heads/{}:{remote_ref}", state.base),
            ],
        ) {
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
        let remote = runner.git(repo, &["rev-parse", &remote_ref])?;
        // Recover a crash or lost push response without another merge or agent call.
        if let Some(commit) = state.merged_commit.clone()
            && runner
                .git(repo, &["merge-base", "--is-ancestor", &commit, &remote])
                .is_ok()
        {
            if require_independent_checks
                && !checks_gate::independent_check_passed(state.independent_check.as_ref(), &commit)
            {
                checks_gate::wait_for_independent_checks(
                    repo,
                    dir,
                    state,
                    &state.worktree.clone(),
                    &commit,
                    runner,
                    policy.auto_publish_gate,
                )?;
            }
            return publication::finish_auto_publish(repo, dir, state, runner);
        }
        let local = if state.destination_branch.is_some() {
            remote.clone()
        } else {
            runner.git(repo, &["rev-parse", "HEAD"])?
        };
        // Integrate on fetched remote truth when histories diverge. The task's
        // verified branch is squash-merged below, with conflicts repaired and
        // verification rerun in isolation. Never rewrite the user's checkout.
        let integration_base = if runner
            .git(repo, &["merge-base", "--is-ancestor", &remote, &local])
            .is_ok()
        {
            local
        } else {
            remote.clone()
        };
        let integration =
            prepare_integration(repo, dir, state, &integration_base, harness, runner, policy)?;
        if !publish_after_integration {
            state.branch = integration.branch.clone();
            state.worktree = integration.worktree.clone();
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
                repo,
                dir,
                state,
                &integration.worktree,
                &verified_commit,
                runner,
                policy.auto_publish_gate,
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
