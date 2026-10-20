use crate::{
    app::session::Project,
    core::{
        implementation,
        lifecycle::{LifecycleStore, TaskPhase},
    },
    harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin},
};
use sha2::Digest;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FindingTarget {
    pub(crate) task_uid: String,
    pub(crate) feature_uid: String,
    pub(crate) subject_hash: String,
    pub(crate) finding_id: String,
    pub(crate) finding_fingerprint: String,
    pub(crate) scope: crate::harness::prompt_catalog::FindingScopeV1,
}

pub(super) fn validate_dispositions(
    actions: &[crate::harness::prompt_catalog::ManagerActionV1],
    targets: &std::collections::HashMap<String, FindingTarget>,
    available_evidence: &std::collections::HashSet<String>,
) -> Result<(), String> {
    let mut seen = std::collections::HashMap::<String, usize>::new();
    for action in actions
        .iter()
        .filter(|action| is_code_review_action(action, targets))
    {
        if action.evidence.len() != 1 {
            return Err("code-review disposition must cite exactly one current finding".into());
        }
        let evidence_id = &action.evidence[0];
        let Some(target) = targets.get(evidence_id) else {
            return Err("code-review disposition cites no current finding".into());
        };
        if !available_evidence.contains(evidence_id)
            || action.target != format!("{}#{}", target.task_uid, target.finding_id)
            || action.expected_revision != target.subject_hash
            || action.reason.trim().len() < 16
            || !valid_action_scope(&action.kind, target.scope)
        {
            return Err("code-review disposition is stale or lacks an actionable rationale".into());
        }
        *seen.entry(evidence_id.clone()).or_default() += 1;
    }
    for evidence_id in targets.keys() {
        if seen.get(evidence_id) != Some(&1) {
            return Err(format!(
                "Manager must return exactly one disposition for current code-review finding {evidence_id}"
            ));
        }
    }
    Ok(())
}

fn is_code_review_action(
    action: &crate::harness::prompt_catalog::ManagerActionV1,
    targets: &std::collections::HashMap<String, FindingTarget>,
) -> bool {
    use crate::harness::prompt_catalog::ManagerActionKind::{
        AcceptRisk, AskUser, ContestFinding, PromoteFollowUp, RepairFinding,
    };
    match &action.kind {
        RepairFinding => true,
        AskUser | AcceptRisk | ContestFinding | PromoteFollowUp => {
            action.evidence.iter().any(|evidence_id| {
                evidence_id.starts_with("code-review-finding:") || targets.contains_key(evidence_id)
            })
        }
        _ => false,
    }
}

pub(in crate::app::manager) fn valid_action_scope(
    kind: &crate::harness::prompt_catalog::ManagerActionKind,
    scope: crate::harness::prompt_catalog::FindingScopeV1,
) -> bool {
    use crate::harness::prompt_catalog::{FindingScopeV1::*, ManagerActionKind::*};
    matches!(
        (kind, scope),
        (RepairFinding | AskUser, Blocking)
            | (PromoteFollowUp, NonBlocking)
            | (AcceptRisk, NonBlocking)
            | (ContestFinding, Blocking)
    )
}

pub(in crate::app::manager) fn current_findings(
    project: &Project,
) -> Result<Vec<(EvidenceItem, FindingTarget)>, String> {
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let mut result = Vec::new();
    for state in project.implementation_states.values() {
        let Some(task_uid) = state.task_uid.as_deref() else {
            continue;
        };
        let Some(current) = lifecycle
            .task_for_planning_item(task_uid)
            .map_err(|error| error.to_string())?
        else {
            continue;
        };
        let Some(reviewed) = current.code_review.as_ref() else {
            continue;
        };
        if current.reviewed_code_head_sha.as_deref() != state.verified_head.as_deref()
            || current.reviewed_code_subject_sha256.as_deref()
                != Some(reviewed.subject_sha256.as_str())
        {
            continue;
        }
        let Some(summary) = implementation::manager_findings_for_task(
            &project.state.repo_root,
            &state.ticket,
            task_uid,
        )
        .map_err(|error| error.to_string())?
        else {
            continue;
        };
        let actionable_review = match (current.phase, summary.pass) {
            (TaskPhase::Repairing, false) => true,
            (TaskPhase::AwaitingRuntimeQa | TaskPhase::AwaitingSpecReconciliation, true) => {
                summary.findings.iter().all(|finding| {
                    finding.scope
                        == Some(crate::harness::prompt_catalog::FindingScopeV1::NonBlocking)
                })
            }
            _ => false,
        };
        if !actionable_review
            || summary.findings.is_empty()
            || summary.subject_hash != reviewed.subject_sha256
            || summary.head_sha != state.verified_head.as_deref().unwrap_or_default()
            || summary.diff_hash != current.implementation_sha256.as_deref().unwrap_or_default()
        {
            continue;
        }
        let task_uid = summary.task_uid;
        let subject_hash = summary.subject_hash;
        let diff_hash = summary.diff_hash;
        let head_sha = summary.head_sha;
        for finding in summary.findings.into_iter().take(64) {
            let identity = format!(
                "{workspace_uid}\0{task_uid}\0{subject_hash}\0{}",
                finding.id
            );
            let digest = format!("{:x}", sha2::Sha256::digest(identity.as_bytes()));
            let evidence_id = format!("code-review-finding:{}", &digest[..24]);
            let content = format!(
                "task_uid={task_uid} subject_sha256={subject_hash} diff_sha256={diff_hash} head_sha={head_sha} finding_id={} requirement={} affected_requirements={:?} severity={:?} scope={:?} kind={:?} suggested_action={:?} rationale={} location={} repro={} correction={} reviewer_evidence_refs={}",
                finding.id,
                finding.requirement,
                finding.affected_requirement_ids,
                finding.severity,
                finding.scope,
                finding.kind,
                finding.suggested_action,
                crate::core::context_build::clip(&finding.rationale, 1024),
                crate::core::context_build::clip(&finding.location, 512),
                crate::core::context_build::clip(
                    finding.repro.as_deref().unwrap_or("not provided"),
                    1024
                ),
                crate::core::context_build::clip(&finding.proposed_correction, 1024),
                finding
                    .evidence
                    .iter()
                    .take(16)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let finding_fingerprint = implementation::finding_fingerprint(&finding);
            result.push((
                EvidenceItem {
                    id: evidence_id,
                    workspace_uid: workspace_uid.clone(),
                    origin: EvidenceOrigin::Application,
                    confidentiality: EvidenceConfidentiality::WorkspacePrivate,
                    source: "validated current-head independent code review finding".into(),
                    content: crate::core::context_build::clip(&content, 4096),
                },
                FindingTarget {
                    task_uid: task_uid.clone(),
                    feature_uid: current.feature_uid.clone(),
                    subject_hash: subject_hash.clone(),
                    finding_id: finding.id,
                    finding_fingerprint,
                    scope: finding.scope.ok_or_else(|| {
                        "current code-review finding is missing its validated scope".to_owned()
                    })?,
                },
            ));
        }
    }
    Ok(result)
}

#[cfg(test)]
#[path = "code_reviews/tests.rs"]
mod tests;
