use super::*;

#[test]
fn code_review_disposition_roundtrips_and_rejects_unbound_work_identity() {
    let root = root();
    let parent_uid = uuid::Uuid::new_v4().to_string();
    let mut item = Work::new(
        "task:code-review-disposition:fixture".into(),
        "Code review repair: finding-1".into(),
        "Repair the cited finding.".into(),
        "Current reviewer evidence is attached.".into(),
    );
    item.kind = WorkKind::CodeReviewDisposition;
    item.status = WorkStatus::Todo;
    item.parent_uid = Some(parent_uid);
    item.code_review_disposition = Some(CodeReviewFindingDispositionV1 {
        finding_id: "finding-1".into(),
        subject_hash: "a".repeat(64),
        scope: Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking),
        finding_fingerprint: Some("c".repeat(64)),
        occurrence_count: 1,
        arbitration_attempts: 0,
        status: CodeReviewDispositionStatusV1::AssignedRepair,
        decision_reason: "The reviewed change misses the required behavior.".into(),
        owner_work_id: item.uid.clone(),
        evidence_ref: "code-review-finding:0123456789abcdef01234567".into(),
    });

    save(&root, &[item.clone()]).unwrap();
    assert_eq!(load(&root).unwrap(), vec![item.clone()]);

    item.code_review_disposition.as_mut().unwrap().owner_work_id = uuid::Uuid::new_v4().to_string();
    assert!(save(&root, &[item]).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
