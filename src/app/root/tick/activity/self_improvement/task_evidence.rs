use super::*;
mod parent_progress;
use parent_progress::synchronize_parent_progress;

use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};
use crate::core::self_improvement::SelfImprovementParentTaskProgressV1;

pub(super) fn current_runtime_qa_pass(receipt: &GateReceipt, subject_sha256: Option<&str>) -> bool {
    receipt.actor == GateActor::IndependentReviewer
        && receipt.decision == GateDecision::Approved
        && receipt.prompt_role()
            == Some(crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV3)
        && subject_sha256.is_some_and(|subject| receipt.subject_sha256 == subject)
        && has_current_scenario_evidence(receipt)
}

fn has_current_scenario_evidence(receipt: &GateReceipt) -> bool {
    let scenario_ref = receipt.evidence_refs.iter().find_map(|reference| {
        let (run_id, hash) = reference
            .strip_prefix("runtime-qa:")?
            .split_once("/scenarios:")?;
        (uuid::Uuid::parse_str(run_id).is_ok() && is_sha256(hash)).then_some(run_id)
    });
    let Some(run_id) = scenario_ref else {
        return false;
    };
    receipt
        .evidence_refs
        .iter()
        .any(|reference| reference == &format!("runtime-qa:{run_id}/result"))
        && receipt.evidence_refs.iter().any(|reference| {
            let Some((evidence_run_id, hash)) = reference
                .strip_prefix("runtime-qa:")
                .and_then(|value| value.split_once("/evidence:"))
            else {
                return false;
            };
            evidence_run_id == run_id && is_sha256(hash)
        })
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn synchronize(
    project: &mut Project,
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
        && run.parent_task_progress.iter().all(|progress| {
            progress.implementation_sha256.is_some() && progress.pr_head_sha.is_some()
        })
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
        && run
            .parent_task_progress
            .iter()
            .all(|progress| progress.code_review_uid.is_some())
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
        && run
            .parent_task_progress
            .iter()
            .all(|progress| progress.runtime_qa_uid.is_some())
        && let Some(receipt) = task.runtime_qa.as_ref()
        && current_runtime_qa_pass(receipt, run.implementation_subject_sha256.as_deref())
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
        && run
            .parent_task_progress
            .iter()
            .all(|progress| progress.spec_receipt_uid.is_some())
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
        && run
            .parent_task_progress
            .iter()
            .all(|progress| progress.pr_work_item_uid.is_some())
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
        && run
            .parent_task_progress
            .iter()
            .all(|progress| progress.pr_url.is_some())
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
        && let Some((task_uid, url)) =
            super::pr_evidence::closed_unmerged_parent(&run.parent_task_progress)
    {
        apply(
            runs,
            run,
            &format!("pr-closed-unmerged:{}:{task_uid}", run.revision),
            Event::PrClosedUnmerged { url },
        )?;
    }
    if run.phase == Phase::PrClosedUnmerged
        && let Some(task_uid) = super::pr_evidence::reopened_parent(&run.parent_task_progress)
    {
        apply(
            runs,
            run,
            &format!("pr-reopened:{}:{task_uid}", run.revision),
            Event::PrReopened,
        )?;
    }
    if run.phase == Phase::PrOpened
        && let Some(sha) = super::pr_evidence::aggregate_merged_parent_sha(
            &run.parent_task_progress,
            run.destination_ref.as_deref(),
        )
        && super::pr_evidence::parent_merge_ancestry_verified(
            &mut project.activity,
            &run.canonical_remote,
            &run.parent_task_progress,
            &sha,
            std::path::Path::new("gh"),
        )?
    {
        apply(runs, run, &format!("merged:{sha}"), Event::Merged { sha })?;
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
