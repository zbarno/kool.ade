use super::{
    QuestionPriority, QuestionStatus, QuestionV1, ReadinessAxis, ReadinessChecklistV1,
    ReadinessResult,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MATERIAL_AXES: [ReadinessAxis; 14] = [
    ReadinessAxis::Who,
    ReadinessAxis::What,
    ReadinessAxis::When,
    ReadinessAxis::Where,
    ReadinessAxis::Why,
    ReadinessAxis::How,
    ReadinessAxis::OutOfScope,
    ReadinessAxis::CriticalPath,
    ReadinessAxis::SystemBoundaries,
    ReadinessAxis::FailureBehavior,
    ReadinessAxis::DataOwnership,
    ReadinessAxis::Authorization,
    ReadinessAxis::MeasurableSuccess,
    ReadinessAxis::TestCases,
];

pub const DEFAULT_NO_PROGRESS_THRESHOLD: u32 = 3;

pub fn assess_readiness(
    questions: &[QuestionV1],
    checklist: &ReadinessChecklistV1,
    required_requirements: &[String],
) -> ReadinessResult {
    let mut invalid = Vec::new();
    if let Err(error) = checklist.validate() {
        invalid.push(error);
    }
    if questions
        .iter()
        .any(|question| question.validate().is_err())
    {
        invalid.push("one or more question records are invalid".into());
    }
    if questions.iter().any(|question| {
        question.feature_uid != checklist.feature_uid
            || question.plan_revision != checklist.plan_revision
    }) {
        invalid.push("question records do not match the assessed feature and plan revision".into());
    }
    if !invalid.is_empty() {
        return ReadinessResult::InvalidEvidence { reasons: invalid };
    }
    if checklist.required_requirement_ids.as_deref() != Some(required_requirements) {
        return ReadinessResult::InvalidEvidence {
            reasons: vec![
                "required requirement IDs do not match the persisted specification snapshot".into(),
            ],
        };
    }
    let axes: BTreeSet<_> = checklist.assessed_axes.iter().cloned().collect();
    let unassessed = MATERIAL_AXES
        .iter()
        .filter(|axis| !axes.contains(axis))
        .cloned()
        .collect::<Vec<_>>();
    let traced: BTreeSet<_> = checklist
        .acceptance_tests
        .iter()
        .map(|entry| entry.requirement_id.as_str())
        .collect();
    let mut untraced_requirements = required_requirements
        .iter()
        .filter(|id| !traced.contains(id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    untraced_requirements.sort();
    untraced_requirements.dedup();
    let mut blockers = questions
        .iter()
        .filter(|q| {
            q.priority == QuestionPriority::Blocking
                && matches!(
                    q.status,
                    QuestionStatus::Draft | QuestionStatus::Open | QuestionStatus::Declined
                )
                && q.deferred.is_none()
        })
        .map(|q| q.uid.clone())
        .collect::<Vec<_>>();
    blockers.extend(checklist.blocker_notes.iter().cloned());
    if checklist.stalled {
        return ReadinessResult::PlanningStalled {
            repeated_fingerprint: checklist.progress_fingerprint.clone().unwrap_or_default(),
        };
    }
    let missing_acceptance_tests = checklist.acceptance_tests.is_empty();
    if blockers.is_empty()
        && unassessed.is_empty()
        && untraced_requirements.is_empty()
        && !missing_acceptance_tests
    {
        ReadinessResult::Ready
    } else {
        ReadinessResult::NeedsWork {
            blockers,
            unassessed,
            untraced_requirements,
            missing_acceptance_tests,
        }
    }
}

pub fn observe_progress(
    checklist: &mut ReadinessChecklistV1,
    questions: &[QuestionV1],
    threshold: u32,
) -> bool {
    let mut stable = questions
        .iter()
        .map(|q| {
            format!(
                "{}:{}:{}:{}",
                q.uid,
                q.revision,
                q.status as u8,
                q.evidence_refs.join(",")
            )
        })
        .collect::<Vec<_>>();
    stable.sort();
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(&checklist.assessed_axes).unwrap_or_default());
    hash.update(serde_json::to_vec(&checklist.acceptance_tests).unwrap_or_default());
    hash.update(serde_json::to_vec(&checklist.evidence_refs).unwrap_or_default());
    hash.update(serde_json::to_vec(&checklist.blocker_notes).unwrap_or_default());
    hash.update(serde_json::to_vec(&checklist.required_requirement_ids).unwrap_or_default());
    hash.update(b"spec_revision\0");
    hash.update(
        checklist
            .spec_revision
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );
    hash.update([0]);
    for item in stable {
        hash.update(item.as_bytes());
        hash.update([0]);
    }
    let fingerprint = format!("{:x}", hash.finalize());
    if checklist.progress_fingerprint.as_deref() == Some(&fingerprint) {
        checklist.no_progress_count = checklist.no_progress_count.saturating_add(1);
    } else {
        checklist.progress_fingerprint = Some(fingerprint);
        checklist.no_progress_count = 0;
    }
    checklist.stalled = threshold > 0 && checklist.no_progress_count >= threshold;
    checklist.stalled
}
