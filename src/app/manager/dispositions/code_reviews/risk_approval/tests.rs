use super::*;
use crate::core::planning_work::{CodeReviewFindingDispositionV1, Work};

fn pending(scope: FindingScopeV1) -> Work {
    let mut work = Work::new(
        "risk-decision".into(),
        "Code review risk decision".into(),
        "Ask the user whether to accept this nonblocking risk.".into(),
        "Current finding evidence".into(),
    );
    work.kind = WorkKind::CodeReviewDisposition;
    work.status = WorkStatus::NeedsAttention;
    work.code_review_disposition = Some(CodeReviewFindingDispositionV1 {
        finding_id: "F-1".into(),
        subject_hash: "a".repeat(64),
        scope: Some(scope),
        finding_fingerprint: Some("b".repeat(64)),
        occurrence_count: 1,
        arbitration_attempts: 0,
        status: CodeReviewDispositionStatusV1::Open,
        decision_reason: "Manager requests an explicit user risk decision.".into(),
        owner_work_id: work.uid.clone(),
        evidence_ref: "code-review-finding:0123456789abcdef01234567".into(),
    });
    work
}

#[test]
fn explicit_user_action_resolves_only_current_nonblocking_risk() {
    let mut work = pending(FindingScopeV1::NonBlocking);
    record_user_acceptance(&mut work, &"a".repeat(64)).unwrap();
    assert_eq!(work.status, WorkStatus::Done);
    let disposition = work.code_review_disposition.unwrap();
    assert_eq!(
        disposition.status,
        CodeReviewDispositionStatusV1::RiskApproved
    );
    assert!(
        disposition
            .decision_reason
            .contains("user explicitly accepted")
    );
}

#[test]
fn risk_acceptance_rejects_blocking_and_stale_decisions() {
    let mut blocking = pending(FindingScopeV1::Blocking);
    assert!(record_user_acceptance(&mut blocking, &"a".repeat(64)).is_err());
    assert_eq!(blocking.status, WorkStatus::NeedsAttention);

    let mut stale = pending(FindingScopeV1::NonBlocking);
    assert!(record_user_acceptance(&mut stale, &"c".repeat(64)).is_err());
    assert_eq!(stale.status, WorkStatus::NeedsAttention);
}
