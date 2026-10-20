use super::*;
use crate::core::planning_work::{
    CodeReviewDispositionStatusV1, CodeReviewFindingDispositionV1, WorkKind,
};

fn disposition(subject: &str, status: CodeReviewDispositionStatusV1) -> Work {
    let mut work = Work::new(
        disposition_key("task-1", &"c".repeat(64)),
        "Repair finding F-1".into(),
        "Repair the finding".into(),
        "Manager disposition".into(),
    );
    work.kind = WorkKind::CodeReviewDisposition;
    work.status = WorkStatus::Done;
    work.parent_uid = Some("00000000-0000-4000-8000-000000000001".into());
    work.code_review_disposition = Some(CodeReviewFindingDispositionV1 {
        finding_id: "F-1".into(),
        subject_hash: subject.into(),
        scope: Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking),
        finding_fingerprint: Some("c".repeat(64)),
        occurrence_count: 1,
        arbitration_attempts: 0,
        status,
        decision_reason: "The reviewed change misses behavior.".into(),
        owner_work_id: work.uid.clone(),
        evidence_ref: "code-review-finding:0123456789abcdef01234567".into(),
    });
    work
}

#[test]
fn finding_key_tracks_same_normalized_issue_across_subjects() {
    assert_eq!(
        disposition_key("task-1", &"c".repeat(64)),
        disposition_key("task-1", &"c".repeat(64))
    );
    assert_ne!(
        disposition_key("task-1", &"c".repeat(64)),
        disposition_key("task-2", &"c".repeat(64))
    );
}

#[test]
fn recurrence_reopens_one_durable_disposition_and_increments_count() {
    let existing = disposition(&"a".repeat(64), CodeReviewDispositionStatusV1::Resolved);
    let mut proposed = disposition(
        &"b".repeat(64),
        CodeReviewDispositionStatusV1::AssignedRepair,
    );
    proposed.status = WorkStatus::Todo;
    let merged = merge(
        &existing,
        proposed,
        "code-review-finding:abcdef0123456789abcdef01",
        "The issue remains on the new head.",
    )
    .unwrap()
    .unwrap();
    let prior = existing.code_review_disposition.as_ref().unwrap();
    let current = merged.code_review_disposition.as_ref().unwrap();
    assert_eq!(merged.uid, existing.uid);
    assert_eq!(current.occurrence_count, prior.occurrence_count + 1);
    assert_eq!(current.subject_hash, "b".repeat(64));
    assert_eq!(
        current.status,
        CodeReviewDispositionStatusV1::AssignedRepair
    );
    assert_eq!(merged.status, WorkStatus::Todo);
    assert!(merged.detail.contains("Recurring finding observed"));
}

#[test]
fn same_subject_duplicate_is_suppressed_without_reopening_work() {
    let existing = disposition(&"a".repeat(64), CodeReviewDispositionStatusV1::Resolved);
    let proposed = disposition(
        &"a".repeat(64),
        CodeReviewDispositionStatusV1::AssignedRepair,
    );
    assert!(
        merge(
            &existing,
            proposed,
            "code-review-finding:abcdef0123456789abcdef01",
            "The issue remains in the same review.",
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn recurrence_cannot_overwrite_an_active_repair() {
    let mut existing = disposition(
        &"a".repeat(64),
        CodeReviewDispositionStatusV1::AssignedRepair,
    );
    existing.status = WorkStatus::InProgress;
    let proposed = disposition(
        &"b".repeat(64),
        CodeReviewDispositionStatusV1::AssignedRepair,
    );
    assert!(
        merge(
            &existing,
            proposed,
            "code-review-finding:abcdef0123456789abcdef01",
            "The issue remains on a new review.",
        )
        .is_err()
    );
}
