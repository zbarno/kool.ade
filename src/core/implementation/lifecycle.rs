use super::*;
use crate::artifacts::planning_store::PlanningStore;

pub(super) struct LifecycleExecutionContext<'a> {
    pub(super) planning_store: &'a PlanningStore,
    pub(super) state_root: &'a Path,
}

pub(super) fn execute(
    context: LifecycleExecutionContext<'_>,
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    policy: ExecutionPolicy<'_>,
) -> anyhow::Result<()> {
    let planning_store = context.planning_store;
    let publication_mode = policy.publication_mode;
    let auto_publish_gate = policy.auto_publish_gate;
    if publication_mode == PublicationMode::CreatePullRequest
        && publication::implementation_is_done(planning_store, state)?
    {
        // PR work resumes from frozen Done evidence. Re-running an author and
        // reviewer here would attempt to mutate an already completed task.
        return prepare_and_publish(planning_store, repo, dir, state, harness, runner, &policy);
    }
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
    integration::integrate_subtasks(planning_store, context.state_root, dir, state, runner)?;
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
    let lifecycle_task = state
        .task_uid
        .as_deref()
        .map(|task_uid| {
            crate::core::lifecycle::LifecycleStore::new(planning_store)
                .task_for_planning_item(task_uid)
        })
        .transpose()?
        .flatten();
    let review_result =
        super::code_review::review_verified(dir, state, lifecycle_task.as_ref(), harness, runner);
    super::code_review::persist_lifecycle_review(
        planning_store,
        dir,
        state,
        runner,
        author_identity.as_ref(),
    )?;
    review_result?;
    if let Some(task_uid) = state.task_uid.as_deref()
        && let Some(task) = crate::core::lifecycle::LifecycleStore::new(planning_store)
            .task_for_planning_item(task_uid)?
        && task.parent_task_uid.is_some()
    {
        state.auto_merge = false;
        state.merged_commit = None;
        state.pr_url = None;
        state.pr_state = None;
        state.status = ImplementationStatus::AwaitingApproval;
        if !state
            .detail
            .contains("Verified subtask retained for its standalone parent")
        {
            state.detail.push_str(
                "\nVerified subtask retained for its standalone parent; its changes will be integrated and re-reviewed in the parent task. Subtasks do not create separate pull requests.",
            );
        }
        return save(dir, state);
    }
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
            prepare_and_publish(planning_store, repo, dir, state, harness, runner, &policy)
        }
    }
}

fn prepare_and_publish(
    planning_store: &PlanningStore,
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
    policy: &ExecutionPolicy<'_>,
) -> anyhow::Result<()> {
    let done = publication::implementation_is_done(planning_store, state)?;
    if done {
        integration::prepare_reviewed_for_pull_request(repo, dir, state, harness, runner, policy)?;
    } else {
        integration::prepare_for_pull_request(repo, dir, state, harness, runner, policy)?;
    }
    publication::prepare_lifecycle_pr_proposal(planning_store, dir, state, harness, runner)?;
    match publication::prepare_lifecycle_publication(planning_store, state)? {
        publication::LifecyclePublicationGate::Legacy => {
            publication::create_pull_request(dir, state, runner, policy.claim_lease)
        }
        publication::LifecyclePublicationGate::Blocked(reason) => {
            runner.update(format!(
                        "PR preparation remains blocked: {reason}. Verified code and review are preserved; no remote changes were made."
                    ));
            if done {
                publication::preserve_done_after_failure(
                    planning_store,
                    dir,
                    state,
                    None,
                    &anyhow::anyhow!(reason),
                )?;
                Ok(())
            } else {
                publication::hold_for_review(dir, state)
            }
        }
        publication::LifecyclePublicationGate::Authorized(binding) => {
            if let Err(error) =
                publication::create_pull_request(dir, state, runner, policy.claim_lease)
            {
                publication::preserve_done_after_failure(
                    planning_store,
                    dir,
                    state,
                    Some(binding.lease_epoch),
                    &error,
                )?;
                Ok(())
            } else {
                match publication::finish_lifecycle_publication(
                    planning_store,
                    state,
                    Some(binding.clone()),
                ) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        publication::preserve_done_after_failure(
                            planning_store,
                            dir,
                            state,
                            Some(binding.lease_epoch),
                            &error,
                        )?;
                        Ok(())
                    }
                }
            }
        }
    }
}
