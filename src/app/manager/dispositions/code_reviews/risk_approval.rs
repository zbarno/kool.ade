use super::super::super::catalog::code_reviews::current_findings;
use crate::{
    app::session::Project,
    core::planning_work::{CodeReviewDispositionStatusV1, WorkKind, WorkStatus},
    harness::prompt_catalog::FindingScopeV1,
};

pub(crate) fn approve_risk(
    project: &mut Project,
    work_id: &str,
    task_uid: &str,
    finding_id: &str,
    subject_hash: &str,
) -> Result<(), String> {
    let (evidence, target) = current_findings(project)?
        .into_iter()
        .find(|(_, target)| {
            target.task_uid == task_uid
                && target.finding_id == finding_id
                && target.subject_hash == subject_hash
                && target.scope == FindingScopeV1::NonBlocking
        })
        .ok_or_else(|| "Risk approval is stale or the finding is not nonblocking.".to_owned())?;
    let index = project
        .planning_work
        .iter()
        .position(|work| work.uid == work_id)
        .ok_or_else(|| "The code-review risk decision no longer exists.".to_owned())?;
    let work = &project.planning_work[index];
    let disposition = work
        .code_review_disposition
        .as_ref()
        .ok_or_else(|| "The selected work item is not a code-review finding.".to_owned())?;
    if work.kind != WorkKind::CodeReviewDisposition
        || work.status != WorkStatus::NeedsAttention
        || work.parent_uid.as_deref() != Some(task_uid)
        || disposition.finding_id != finding_id
        || disposition.subject_hash != subject_hash
        || disposition.scope != Some(FindingScopeV1::NonBlocking)
        || disposition.finding_fingerprint.as_deref() != Some(target.finding_fingerprint.as_str())
        || disposition.evidence_ref != evidence.id
        || disposition.status != CodeReviewDispositionStatusV1::Open
    {
        return Err("The code-review risk decision is no longer current.".into());
    }
    let original = project.planning_work.clone();
    record_user_acceptance(&mut project.planning_work[index], subject_hash)?;
    if let Err(error) = project.save_planning_work() {
        project.planning_work = original;
        return Err(error);
    }
    Ok(())
}

fn record_user_acceptance(
    work: &mut crate::core::planning_work::Work,
    subject_hash: &str,
) -> Result<(), String> {
    let disposition = work
        .code_review_disposition
        .as_mut()
        .ok_or_else(|| "The selected work item is not a code-review finding.".to_owned())?;
    if work.kind != WorkKind::CodeReviewDisposition
        || work.status != WorkStatus::NeedsAttention
        || disposition.status != CodeReviewDispositionStatusV1::Open
        || disposition.subject_hash != subject_hash
        || disposition.scope != Some(FindingScopeV1::NonBlocking)
    {
        return Err("Only a current nonblocking risk decision can be accepted.".into());
    }
    disposition.status = CodeReviewDispositionStatusV1::RiskApproved;
    disposition.decision_reason =
        "The user explicitly accepted this nonblocking residual risk in Task Details.".into();
    work.status = WorkStatus::Done;
    work.detail.push_str(
        "\n\nUser decision: explicitly accepted this nonblocking residual risk for the exact current review subject.",
    );
    Ok(())
}

#[cfg(test)]
#[path = "risk_approval/tests.rs"]
mod tests;
