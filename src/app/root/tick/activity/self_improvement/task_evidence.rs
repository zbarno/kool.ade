use super::*;
use crate::core::self_improvement::SelfImprovementParentTaskProgressV1;

pub(super) fn synchronize(
    project: &Project,
    lifecycle: &LifecycleStore<'_>,
    runs: &SelfImprovementStore<'_>,
    run: &mut crate::core::self_improvement::SelfImprovementRunV1,
) -> Result<(), String> {
    synchronize_parent_progress(project, lifecycle, runs, run)?;
    let Some(owner_uid) = run.owner_task_uid.as_deref() else {
        return Ok(());
    };
    let task = lifecycle
        .load_task(owner_uid)
        .map_err(|error| error.to_string())?;
    if task.phase == crate::core::lifecycle::TaskPhase::Cancelled
        && !matches!(
            run.phase,
            Phase::PrOpened | Phase::MergedNotActivated | Phase::Activated | Phase::Cancelled
        )
    {
        apply(
            runs,
            run,
            &format!("cancelled-task:{owner_uid}"),
            Event::Cancel,
        )?;
        return Ok(());
    }
    if let Some(implementation) = implementation_for(project, &task)
        && run.phase == Phase::Implementing
        && let (Some(head_sha), Some(subject_sha256)) = (
            implementation.verified_head.as_ref(),
            task.implementation_sha256.as_ref(),
        )
    {
        apply(
            runs,
            run,
            &format!("implementation:{head_sha}:{subject_sha256}"),
            Event::ImplementationFinished {
                head_sha: head_sha.clone(),
                subject_sha256: subject_sha256.clone(),
            },
        )?;
    }
    if run.phase == Phase::Reviewing
        && let Some(receipt) = task.code_review.as_ref()
        && receipt.actor == GateActor::IndependentReviewer
        && receipt.decision == GateDecision::Approved
        && receipt.prompt_role() == Some(crate::harness::prompt_catalog::PromptRole::CodeReviewer)
        && Some(receipt.subject_sha256.as_str()) == run.implementation_subject_sha256.as_deref()
    {
        apply(
            runs,
            run,
            &format!("code-review:{}", receipt.receipt_uid),
            Event::CodeReviewPassed {
                receipt_uid: receipt.receipt_uid.clone(),
                subject_sha256: receipt.subject_sha256.clone(),
            },
        )?;
    }
    if run.phase == Phase::Qa
        && let Some(receipt) = task.runtime_qa.as_ref()
        && matches!(
            receipt.decision,
            GateDecision::Approved | GateDecision::NotApplicable
        )
        && Some(receipt.subject_sha256.as_str()) == run.implementation_subject_sha256.as_deref()
    {
        apply(
            runs,
            run,
            &format!("runtime-qa:{}", receipt.receipt_uid),
            Event::QaPassed {
                receipt_uid: receipt.receipt_uid.clone(),
                subject_sha256: receipt.subject_sha256.clone(),
            },
        )?;
    }
    if run.phase == Phase::SpecSync
        && let Some(receipt) = task.spec_reconciliation.as_ref()
        && receipt.actor == GateActor::ApplicationPolicy
        && receipt.decision == GateDecision::Approved
        && Some(receipt.subject_sha256.as_str()) == run.implementation_subject_sha256.as_deref()
    {
        apply(
            runs,
            run,
            &format!("spec:{}", receipt.receipt_uid),
            Event::SpecReconciled {
                receipt_uid: receipt.receipt_uid.clone(),
                subject_sha256: receipt.subject_sha256.clone(),
            },
        )?;
    }
    if run.phase == Phase::DoneAwaitingPr
        && let Some(work_item_uid) = task.pr_work_item_uid.as_ref()
    {
        apply(
            runs,
            run,
            &format!("pr-prepared:{work_item_uid}"),
            Event::PrPrepared {
                work_item_uid: work_item_uid.clone(),
            },
        )?;
    }
    if run.phase == Phase::PrPending
        && let Some(implementation) = implementation_for(project, &task)
        && let Some(url) = implementation.pr_url.as_ref()
    {
        apply(
            runs,
            run,
            &format!("pr-opened:{url}"),
            Event::PrOpened { url: url.clone() },
        )?;
    }
    if run.phase == Phase::PrOpened
        && let Some(implementation) = implementation_for(project, &task)
        && implementation
            .pr_state
            .is_some_and(|state| state == crate::core::implementation::PullRequestState::Merged)
        && let Some(sha) = implementation.merged_commit.as_ref()
    {
        apply(
            runs,
            run,
            &format!("merged:{sha}"),
            Event::Merged { sha: sha.clone() },
        )?;
    }
    Ok(())
}

fn synchronize_parent_progress(
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
        let progress = SelfImprovementParentTaskProgressV1 {
            task_uid: task.task_uid.clone(),
            phase: task.phase,
            implementation_sha256: task.implementation_sha256.clone(),
            code_review_uid: task
                .code_review
                .as_ref()
                .map(|receipt| receipt.receipt_uid.clone()),
            runtime_qa_uid: task
                .runtime_qa
                .as_ref()
                .map(|receipt| receipt.receipt_uid.clone()),
            spec_receipt_uid: task
                .spec_reconciliation
                .as_ref()
                .map(|receipt| receipt.receipt_uid.clone()),
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

pub(super) fn implementation_for<'a>(
    project: &'a Project,
    task: &crate::core::lifecycle::TaskRecord,
) -> Option<&'a crate::core::implementation::Implementation> {
    project.implementation_states.values().find(|state| {
        state.task_repository_kind == TaskRepositoryKind::Clone
            && state.task_uid.as_deref().is_some_and(|uid| {
                uid == task.task_uid || task.planning_item_uid.as_deref() == Some(uid)
            })
            && state.task_repositories.contains(&state.task_repository)
    })
}
