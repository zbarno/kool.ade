use super::*;

pub(super) fn synchronize_parent_progress(
    project: &Project,
    lifecycle: &LifecycleStore<'_>,
    runs: &SelfImprovementStore<'_>,
    run: &mut crate::core::self_improvement::SelfImprovementRunV1,
) -> Result<(), String> {
    let parent_uids = run
        .parent_task_progress
        .iter()
        .map(|progress| progress.task_uid.clone())
        .collect::<Vec<_>>();
    for task_uid in parent_uids {
        let task = lifecycle
            .load_task(&task_uid)
            .map_err(|error| error.to_string())?;
        let implementation = implementation_for(project, &task);
        let pr = task
            .pr_work_item_uid
            .as_deref()
            .map(|uid| {
                lifecycle
                    .load_pr_preparation(uid)
                    .map_err(|error| error.to_string())
            })
            .transpose()?;
        let implementation_sha256 = implementation
            .filter(|state| state.verified_head.is_some())
            .and_then(|_| task.implementation_sha256.clone());
        let code_review_uid = task.code_review.as_ref().and_then(|receipt| {
            (receipt.actor == GateActor::IndependentReviewer
                && receipt.decision == GateDecision::Approved
                && receipt.prompt_role()
                    == Some(crate::harness::prompt_catalog::PromptRole::CodeReviewer)
                && Some(receipt.subject_sha256.as_str()) == implementation_sha256.as_deref())
            .then(|| receipt.receipt_uid.clone())
        });
        let runtime_qa_uid = task.runtime_qa.as_ref().and_then(|receipt| {
            current_runtime_qa_pass(receipt, implementation_sha256.as_deref())
                .then(|| receipt.receipt_uid.clone())
        });
        let spec_receipt_uid = task.spec_reconciliation.as_ref().and_then(|receipt| {
            (receipt.actor == GateActor::ApplicationPolicy
                && receipt.decision == GateDecision::Approved
                && Some(receipt.subject_sha256.as_str()) == implementation_sha256.as_deref())
            .then(|| receipt.receipt_uid.clone())
        });
        let progress = SelfImprovementParentTaskProgressV1 {
            task_uid: task.task_uid.clone(),
            phase: task.phase,
            implementation_sha256,
            code_review_uid,
            runtime_qa_uid,
            spec_receipt_uid,
            pr_work_item_uid: pr.as_ref().map(|record| record.work_item_uid.clone()),
            pr_status: pr.as_ref().map(|record| record.status),
            pr_url: pr
                .as_ref()
                .and_then(|record| record.provider_pr_url.clone())
                .or_else(|| implementation.and_then(|state| state.pr_url.clone())),
            pr_head_sha: pr
                .as_ref()
                .and_then(|record| record.head_sha.clone())
                .or_else(|| implementation.and_then(|state| state.verified_head.clone())),
            pr_base_branch: implementation.and_then(|state| {
                state
                    .pr_details
                    .as_ref()
                    .map(|details| details.base_branch.clone())
            }),
            pr_merged_commit_sha: pr
                .as_ref()
                .and_then(|record| record.merged_commit_sha.clone()),
            pr_merged_at: implementation
                .and_then(|state| state.pr_details.as_ref()?.merged_at.clone()),
        };
        if run
            .parent_task_progress
            .iter()
            .find(|current| current.task_uid == task_uid)
            != Some(&progress)
        {
            let revision = pr.as_ref().map_or(0, |record| record.revision);
            let event_uid = super::transition_uid(
                &run.run_uid,
                &format!("parent-progress:{task_uid}:{}:{revision}", task.revision),
            )?;
            *run = runs
                .apply_event(
                    &run.run_uid,
                    run.revision,
                    &event_uid,
                    Event::ParentTaskProgress(progress),
                )
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}
