use super::*;

#[test]
fn only_current_subject_follow_up_or_explicit_risk_approval_satisfies_nonblocking_disposition() {
    let target = FindingTarget {
        task_uid: "00000000-0000-4000-8000-000000000001".into(),
        feature_uid: "00000000-0000-4000-8000-000000000002".into(),
        subject_hash: "a".repeat(64),
        finding_id: "F-1".into(),
        finding_fingerprint: "c".repeat(64),
        scope: crate::harness::prompt_catalog::FindingScopeV1::NonBlocking,
    };
    let mut work = Work::new(
        "follow-up".into(),
        "Follow up".into(),
        "Request".into(),
        "Detail".into(),
    );
    work.code_review_disposition = Some(CodeReviewFindingDispositionV1 {
        finding_id: target.finding_id.clone(),
        subject_hash: target.subject_hash.clone(),
        scope: Some(target.scope),
        finding_fingerprint: Some(target.finding_fingerprint.clone()),
        occurrence_count: 1,
        arbitration_attempts: 0,
        status: CodeReviewDispositionStatusV1::PromotedFollowUp,
        decision_reason: "This is an independent improvement.".into(),
        owner_work_id: work.uid.clone(),
        evidence_ref: "code-review-finding:0123456789abcdef01234567".into(),
    });
    assert!(has_terminal_nonblocking_disposition(
        std::slice::from_ref(&work),
        &target
    ));

    let mut stale = target.clone();
    stale.subject_hash = "b".repeat(64);
    assert!(!has_terminal_nonblocking_disposition(
        std::slice::from_ref(&work),
        &stale
    ));
    work.code_review_disposition.as_mut().unwrap().status =
        CodeReviewDispositionStatusV1::RiskApproved;
    assert!(has_terminal_nonblocking_disposition(
        std::slice::from_ref(&work),
        &target
    ));
    work.code_review_disposition.as_mut().unwrap().status =
        CodeReviewDispositionStatusV1::AssignedRepair;
    assert!(!has_terminal_nonblocking_disposition(
        std::slice::from_ref(&work),
        &target
    ));
}
