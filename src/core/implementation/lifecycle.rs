use super::*;

pub(super) fn execute(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    policy: ExecutionPolicy<'_>,
) -> anyhow::Result<()> {
    let ExecutionPolicy {
        user_context,
        publication_mode,
        require_independent_checks,
        auto_publish_gate,
    } = policy;
    let require_independent_checks =
        require_independent_checks || publication_mode == PublicationMode::AutoPublish;
    verification::prepare_verified(repo, dir, state, harness, runner, user_context)?;
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
        PublicationMode::AutoPublish => integration::auto_publish(
            repo,
            dir,
            state,
            harness,
            runner,
            auto_publish_gate,
            require_independent_checks,
        ),
        PublicationMode::CreatePullRequest => publication::create_pull_request(dir, state, runner),
    }
}
