use super::*;
use crate::artifacts::planning_store::PlanningStore;
use sha2::{Digest, Sha256};

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
        if reopen_if_publication_head_changed(planning_store, dir, state, runner)? {
            state.status = ImplementationStatus::AwaitingApproval;
            state.auto_merge = false;
            state.detail = "Destination integration changed the reviewed head. The old review, QA, and specification receipts were invalidated. Refresh those gates before authorizing PR publication.".into();
            save(dir, state)?;
            runner.update("Destination integration changed the reviewed head. The old review, QA, and specification gates were invalidated; refresh them before PR publication.");
            return Ok(());
        }
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

fn reopen_if_publication_head_changed(
    planning_store: &PlanningStore,
    dir: &Path,
    state: &Implementation,
    runner: &Runner,
) -> anyhow::Result<bool> {
    let (Some(planning_uid), Some(head_sha)) =
        (state.task_uid.as_deref(), state.verified_head.as_deref())
    else {
        return Ok(false);
    };
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(planning_store);
    let Some(task) = lifecycle.task_for_planning_item(planning_uid)? else {
        return Ok(false);
    };
    let Some(work_uid) = task.pr_work_item_uid.as_deref() else {
        return Ok(false);
    };
    let work = lifecycle.load_pr_preparation(work_uid)?;
    if task.phase != crate::core::lifecycle::TaskPhase::Done
        || task.reviewed_code_head_sha.as_deref() == Some(head_sha)
        || work.provider_pr_url.is_some()
        || !matches!(
            work.status,
            crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization
                | crate::core::lifecycle::PullRequestPreparationStatus::NeedsAttention
        )
    {
        return Ok(false);
    }

    const MAX_REVIEW_DIFF_BYTES: u64 = 48 * 1024;
    let patch_path = dir.join("publication-head-change.patch");
    runner.git_to_file_bounded(
        &state.task_repository,
        &[
            "diff",
            "--binary",
            &format!("{}...{head_sha}", state.base_commit),
        ],
        &patch_path,
        MAX_REVIEW_DIFF_BYTES,
    )?;
    let code_sha256 = format!("{:x}", Sha256::digest(std::fs::read(patch_path)?));
    lifecycle.apply_task_event(
        &task.task_uid,
        task.revision,
        &uuid::Uuid::new_v5(
            &uuid::Uuid::parse_str(&task.task_uid)?,
            format!("publication-head-changed:v1:{head_sha}:{code_sha256}").as_bytes(),
        )
        .hyphenated()
        .to_string(),
        crate::core::lifecycle::TaskEvent::PublicationHeadChanged {
            code_sha256,
            head_sha: head_sha.to_owned(),
        },
    )?;
    Ok(true)
}
