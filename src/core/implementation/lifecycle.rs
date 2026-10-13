use super::*;
use crate::artifacts::planning_store::PlanningStore;

pub(super) fn execute(
    planning_store: &PlanningStore,
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    policy: ExecutionPolicy<'_>,
) -> anyhow::Result<()> {
    let publication_mode = policy.publication_mode;
    let auto_publish_gate = policy.auto_publish_gate;
    if dir.join("base-reconciliation.json").exists() {
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
    let author_identity = harness.current_execution_identity();
    let review_result = super::code_review::review_verified(dir, state, harness, runner);
    super::code_review::persist_lifecycle_review(
        planning_store,
        dir,
        state,
        runner,
        author_identity.as_ref(),
    )?;
    review_result?;
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
            integration::prepare_for_pull_request(repo, dir, state, harness, runner, &policy)?;
            match publication::prepare_lifecycle_publication(planning_store, state)? {
                publication::LifecyclePublicationGate::Legacy => {
                    publication::create_pull_request(dir, state, runner, policy.claim_lease)
                }
                publication::LifecyclePublicationGate::Blocked(reason) => {
                    runner.update(format!(
                        "PR preparation remains blocked: {reason}. Verified code and review are preserved; no remote changes were made."
                    ));
                    publication::hold_for_review(dir, state)
                }
                publication::LifecyclePublicationGate::Authorized(binding) => {
                    publication::create_pull_request(dir, state, runner, policy.claim_lease)?;
                    publication::finish_lifecycle_publication(planning_store, state, Some(binding))
                }
            }
        }
    }
}
