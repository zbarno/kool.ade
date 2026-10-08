use super::*;

pub(super) fn execute(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    policy: ExecutionPolicy<'_>,
) -> anyhow::Result<()> {
    let publication_mode = policy.publication_mode;
    let auto_publish_gate = policy.auto_publish_gate;
    if state.task_repository_kind == TaskRepositoryKind::LegacyWorktree
        || dir.join("base-reconciliation.json").exists()
    {
        super::initial_reconciliation::prepare(
            repo,
            dir,
            state,
            harness,
            runner,
            policy.user_context,
            policy.accrual.as_ref(),
        )?;
    }
    verification::prepare_verified(
        repo,
        dir,
        state,
        harness,
        runner,
        policy.user_context,
        policy.accrual.as_ref(),
    )?;
    if state.status == ImplementationStatus::Completed {
        return Ok(());
    }
    match publication_mode {
        PublicationMode::HoldForReview => {
            runner.update(
                "Verified and saved locally. Auto Publish is off; no remote changes were made.",
            );
            publication::hold_for_review(dir, state)
        }
        PublicationMode::AutoPublish if !publication::auto_publish_enabled(auto_publish_gate) => {
            runner.update(
                "Auto Publish was turned off before publication. Verified work is saved locally.",
            );
            publication::hold_for_review(dir, state)
        }
        PublicationMode::AutoPublish => {
            integration::auto_publish(repo, dir, state, harness, runner, &policy)
        }
        PublicationMode::CreatePullRequest => {
            if state.source_branch.is_some() || state.destination_branch.is_some() {
                integration::prepare_for_pull_request(repo, dir, state, harness, runner, &policy)?;
            }
            publication::create_pull_request(dir, state, runner)
        }
    }
}
