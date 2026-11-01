use super::*;

fn valid_context() -> ManagerRepairContextV1 {
    ManagerRepairContextV1 {
        work_id: "00000000-0000-4000-8000-000000000001".into(),
        task_uid: "00000000-0000-4000-8000-000000000002".into(),
        finding_id: "finding-17".into(),
        finding_fingerprint: Some("c".repeat(64)),
        reviewed_subject_hash: "a".repeat(64),
        reviewed_head_sha: "b".repeat(40),
        evidence_ref: "code-review-finding:0123456789abcdef01234567".into(),
        manager_reason: "The finding identifies a concrete missing acceptance criterion.".into(),
        finding_record: "Blocking correctness issue in src/lib.rs".into(),
    }
}

#[test]
fn manager_repair_context_accepts_bounded_exact_task_evidence() {
    assert!(valid_context().validate().is_ok());
}

#[test]
fn manager_repair_context_rejects_invalid_identity_and_unbounded_evidence() {
    let mut invalid = valid_context();
    invalid.task_uid = "task-1".into();
    assert!(invalid.validate().is_err());

    let mut invalid = valid_context();
    invalid.reviewed_head_sha = "not-a-commit".into();
    assert!(invalid.validate().is_err());

    let mut invalid = valid_context();
    invalid.evidence_ref = "code-review-finding:not-hexadecimal-00000000".into();
    assert!(invalid.validate().is_err());

    let mut invalid = valid_context();
    invalid.finding_record = "x".repeat(8193);
    assert!(invalid.validate().is_err());
}

#[test]
fn manager_repair_context_accepts_persisted_runtime_qa_identity() {
    let finding_id = "00000000-0000-4000-8000-000000000003";
    let context = ManagerRepairContextV1 {
        finding_id: finding_id.into(),
        evidence_ref: format!("runtime-qa-finding:{finding_id}"),
        finding_fingerprint: None,
        finding_record: r#"{"smallest_fix":"restore expected behavior"}"#.into(),
        ..valid_context()
    };
    assert!(context.is_runtime_qa());
    assert!(context.validate().is_ok());

    let invalid = ManagerRepairContextV1 {
        evidence_ref: "runtime-qa-finding:not-a-uuid".into(),
        ..context
    };
    assert!(invalid.validate().is_err());
}
