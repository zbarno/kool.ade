use super::super::catalog::code_reviews::FindingTarget;
use crate::{
    app::session::Project,
    core::planning_work::{
        CodeReviewDispositionStatusV1, CodeReviewFindingDispositionV1, Work, WorkKind, WorkStatus,
    },
    harness::prompt_catalog::{ManagerActionKind, ManagerActionV1},
};
use sha2::Digest;

#[path = "code_reviews/recurrence.rs"]
mod recurrence;
#[path = "code_reviews/repair.rs"]
mod repair;
#[path = "code_reviews/risk_approval.rs"]
mod risk_approval;
pub(crate) use repair::finish_repair;
pub(crate) use repair::mark_repair_failed;
pub(crate) use repair::next_repair;
pub(crate) use repair::{CodeReviewArbitrationRequest, finish_arbitration, next_arbitration};
pub(crate) use risk_approval::approve_risk;

pub(super) fn prepare(
    project: &Project,
    actions: &[ManagerActionV1],
    expected_evidence_hashes: &std::collections::HashMap<String, String>,
    targets: &std::collections::HashMap<String, FindingTarget>,
) -> Result<Vec<Work>, String> {
    if !actions
        .iter()
        .any(|action| is_code_review_action(action, targets))
    {
        return Ok(Vec::new());
    }
    let current = super::super::catalog::code_reviews::current_findings(project)?;
    let current = current
        .into_iter()
        .map(|(item, target)| (item.id.clone(), (item, target)))
        .collect::<std::collections::HashMap<_, _>>();
    let mut work = Vec::new();
    for action in actions {
        if !is_code_review_action(action, targets) {
            continue;
        }
        let disposition = match &action.kind {
            ManagerActionKind::RepairFinding => Some((
                "repair",
                "Code review repair",
                "Repair only the cited current code-review finding. Preserve the approved specification, make the smallest scoped change, and produce fresh verification plus independent review.",
                WorkStatus::Todo,
                CodeReviewDispositionStatusV1::AssignedRepair,
            )),
            ManagerActionKind::AskUser => Some((
                "ask-user",
                "Code review decision",
                "Resolve the user decision identified by this current code-review finding before changing product scope.",
                WorkStatus::NeedsAttention,
                CodeReviewDispositionStatusV1::Open,
            )),
            ManagerActionKind::AcceptRisk => Some((
                "accept-risk",
                "Code review risk decision",
                "Ask the user whether to accept this nonblocking residual code-review risk. This card is only a proposal; the risk remains open until the user explicitly accepts it in Task Details.",
                WorkStatus::NeedsAttention,
                CodeReviewDispositionStatusV1::Open,
            )),
            ManagerActionKind::PromoteFollowUp => Some((
                "follow-up",
                "Code review follow-up",
                "Plan this distinct nonblocking improvement separately from the reviewed task.",
                WorkStatus::Todo,
                CodeReviewDispositionStatusV1::PromotedFollowUp,
            )),
            ManagerActionKind::ContestFinding => Some((
                "contest",
                "Independent code-review arbitration",
                "Request a new independent review of the exact frozen code subject and the cited contested finding. The contest does not resolve or waive the finding.",
                WorkStatus::NeedsAttention,
                CodeReviewDispositionStatusV1::Contested,
            )),
            _ => None,
        };
        let Some((kind, title, request, status, disposition_status)) = disposition else {
            continue;
        };
        if action.evidence.len() != 1 {
            return Err("code-review disposition must cite exactly one current finding".into());
        }
        let evidence_id = &action.evidence[0];
        let Some(target) = targets.get(evidence_id) else {
            return Err("code-review disposition cites a finding that is no longer current".into());
        };
        let Some((item, current_target)) = current.get(evidence_id) else {
            return Err("code-review finding changed before its disposition was saved".into());
        };
        let current_hash = format!("{:x}", sha2::Sha256::digest(item.content.as_bytes()));
        if current_target != target
            || action.target != format!("{}#{}", target.task_uid, target.finding_id)
            || action.expected_revision != target.subject_hash
            || expected_evidence_hashes.get(evidence_id) != Some(&current_hash)
            || action.reason.trim().len() < 16
            || !super::super::catalog::code_reviews::valid_action_scope(&action.kind, target.scope)
        {
            return Err("code-review disposition is stale or lacks an actionable rationale".into());
        }
        let key = recurrence::disposition_key(&target.task_uid, &target.finding_fingerprint);
        let reason = crate::app::root::plan_review::verdict::sanitize_text(&action.reason, 2048);
        let detail = format!(
            "Manager disposition: {kind}\nReview subject SHA-256: {}\nFinding evidence: {evidence_id}\nFinding record:\n{}\nManager rationale: {reason}",
            target.subject_hash,
            crate::core::context_build::clip(&item.content, 4096),
        );
        let mut record = Work::new(
            key.clone(),
            format!("{title}: {}", target.finding_id),
            request.into(),
            detail,
        );
        record.kind = WorkKind::CodeReviewDisposition;
        record.status = status;
        record.feature_uid = Some(target.feature_uid.clone());
        record.parent_uid = Some(target.task_uid.clone());
        record.code_review_disposition = Some(CodeReviewFindingDispositionV1 {
            finding_id: target.finding_id.clone(),
            subject_hash: target.subject_hash.clone(),
            scope: Some(target.scope),
            finding_fingerprint: Some(target.finding_fingerprint.clone()),
            occurrence_count: 1,
            arbitration_attempts: 0,
            status: disposition_status,
            decision_reason: reason.clone(),
            owner_work_id: record.uid.clone(),
            evidence_ref: evidence_id.clone(),
        });
        if let Some(existing) = work
            .iter()
            .chain(project.planning_work.iter())
            .find(|item| {
                item.key == key
                    || item.parent_uid.as_deref() == Some(target.task_uid.as_str())
                        && item
                            .code_review_disposition
                            .as_ref()
                            .is_some_and(|disposition| {
                                disposition.finding_fingerprint.as_deref()
                                    == Some(target.finding_fingerprint.as_str())
                            })
            })
        {
            if let Some(recurring) = recurrence::merge(existing, record, evidence_id, &reason)? {
                work.retain(|item| item.key != key);
                work.push(recurring);
            }
            continue;
        }
        work.push(record);
    }
    Ok(work)
}

fn is_code_review_action(
    action: &ManagerActionV1,
    targets: &std::collections::HashMap<String, FindingTarget>,
) -> bool {
    match &action.kind {
        ManagerActionKind::RepairFinding => true,
        ManagerActionKind::AskUser
        | ManagerActionKind::AcceptRisk
        | ManagerActionKind::PromoteFollowUp
        | ManagerActionKind::ContestFinding => action.evidence.iter().any(|evidence_id| {
            evidence_id.starts_with("code-review-finding:") || targets.contains_key(evidence_id)
        }),
        _ => false,
    }
}

pub(crate) fn has_unpromoted_nonblocking_findings(
    project: &Project,
    task_uid: &str,
) -> Result<bool, String> {
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
    let task = lifecycle
        .task_for_planning_item(task_uid)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "lifecycle task is missing during code-review disposition check".to_owned()
        })?;
    if task.phase != crate::core::lifecycle::TaskPhase::AwaitingSpecReconciliation {
        return Ok(false);
    }
    let state = project
        .implementation_states
        .values()
        .find(|state| state.task_uid.as_deref() == Some(task_uid))
        .ok_or_else(|| {
            "implementation state is missing for the current reviewed task".to_owned()
        })?;
    let summary = crate::core::implementation::manager_findings_for_task(
        &project.state.repo_root,
        &state.ticket,
        task_uid,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "validated current-head code-review receipt is missing".to_owned())?;
    if !summary.pass
        || summary.head_sha != task.reviewed_code_head_sha.as_deref().unwrap_or_default()
        || summary.subject_hash
            != task
                .reviewed_code_subject_sha256
                .as_deref()
                .unwrap_or_default()
        || summary.diff_hash != task.implementation_sha256.as_deref().unwrap_or_default()
        || summary.findings.iter().any(|finding| {
            finding.scope != Some(crate::harness::prompt_catalog::FindingScopeV1::NonBlocking)
        })
    {
        return Err("code-review receipt does not match the passing lifecycle subject".into());
    }
    let current = super::super::catalog::code_reviews::current_findings(project)?;
    Ok(current.into_iter().any(|(_, target)| {
        target.task_uid == task_uid
            && target.scope == crate::harness::prompt_catalog::FindingScopeV1::NonBlocking
            && !has_terminal_nonblocking_disposition(&project.planning_work, &target)
    }))
}

fn has_terminal_nonblocking_disposition(work_items: &[Work], target: &FindingTarget) -> bool {
    work_items.iter().any(|work| {
        work.code_review_disposition
            .as_ref()
            .is_some_and(|disposition| {
                disposition.finding_id == target.finding_id
                    && disposition.subject_hash == target.subject_hash
                    && matches!(
                        disposition.status,
                        CodeReviewDispositionStatusV1::PromotedFollowUp
                            | CodeReviewDispositionStatusV1::RiskApproved
                    )
            })
    })
}

#[cfg(test)]
#[path = "code_reviews/tests.rs"]
mod tests;
