use super::*;

#[path = "tests/persisted_v1_retry.rs"]
mod persisted_v1_retry;
#[path = "tests/reviewer_failure.rs"]
mod reviewer_failure;

pub(super) fn receipt() -> CodeReviewReceiptV1 {
    receipt_for_role(crate::harness::prompt_catalog::PromptRole::CodeReviewer)
}

fn receipt_for_role(role: crate::harness::prompt_catalog::PromptRole) -> CodeReviewReceiptV1 {
    use crate::harness::prompt_catalog::{
        EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, RoleInputV1, RolePayloadV1,
    };
    let subject_hash = "a".repeat(64);
    let rendered = crate::harness::prompt_catalog::render(&RoleInputV1 {
        role,
        workspace_uid: "workspace-1".into(),
        feature_uid: "00000000-0000-4000-8000-000000000001".into(),
        task_uid: Some("00000000-0000-4000-8000-000000000002".into()),
        original_goal: "Implement a bounded behavior".into(),
        approved_spec_ref: Some("spec-v1".into()),
        approved_spec_hash: Some("b".repeat(64)),
        decision_refs: Vec::new(),
        evidence_manifest: vec![EvidenceItem {
            id: "verified-test-report-0".into(),
            workspace_uid: "workspace-1".into(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: "validated test report".into(),
            content: "all required checks passed".into(),
        }],
        execution_policy_ref: "review/read-only/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: "base...head".into(),
            subject_hash: subject_hash.clone(),
            author_run_id: "00000000-0000-4000-8000-000000000003".into(),
            reviewer_run_id: "00000000-0000-4000-8000-000000000004".into(),
            author_session_id: Some("00000000-0000-4000-8000-000000000005".into()),
            reviewer_session_id: Some("00000000-0000-4000-8000-000000000006".into()),
            requirements_matrix: vec!["AC-1".into()],
            explicit_exclusions: Vec::new(),
        },
    })
    .unwrap();
    CodeReviewReceiptV1 {
        schema_version: 1,
        task_uid: "00000000-0000-4000-8000-000000000002".into(),
        feature_uid: "00000000-0000-4000-8000-000000000001".into(),
        subject_hash: subject_hash.clone(),
        diff_hash: "c".repeat(64),
        base_sha: "d".repeat(40),
        head_sha: "e".repeat(40),
        author_run_id: "00000000-0000-4000-8000-000000000003".into(),
        author_session_id: "00000000-0000-4000-8000-000000000005".into(),
        reviewer_run_id: "00000000-0000-4000-8000-000000000004".into(),
        reviewer_session_id: "00000000-0000-4000-8000-000000000006".into(),
        prompt: rendered.manifest,
        requirements: vec!["AC-1".into()],
        affected_paths: Vec::new(),
        evidence_ids: vec!["verified-test-report-0".into()],
        verdict: crate::harness::prompt_catalog::ReviewVerdictV1 {
            subject_hash,
            examined_files: Vec::new(),
            tested_evidence_ids: vec!["verified-test-report-0".into()],
            unexamined_areas: Vec::new(),
            pass: true,
            outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass),
            coverage: vec![crate::harness::prompt_catalog::CoverageV1 {
                requirement: "AC-1".into(),
                status: crate::harness::prompt_catalog::CoverageStatus::Pass,
                evidence: vec!["verified-test-report-0".into()],
            }],
            findings: Vec::new(),
            untested: Vec::new(),
        },
    }
}

#[test]
fn legacy_v1_receipts_remain_readable_but_do_not_meet_the_v2_approval_rule() {
    let mut value = receipt_for_role(crate::harness::prompt_catalog::PromptRole::CodeReviewerV1);
    value.verdict.pass = true;
    value
        .verdict
        .findings
        .push(crate::harness::prompt_catalog::ReviewFindingV1 {
            id: "finding-1".into(),
            requirement: "AC-1".into(),
            location_detail: None,
            rationale: "The reviewer reproduced the unnecessary change.".into(),
            affected_requirement_ids: vec!["AC-1".into()],
            suggested_action: Some(crate::harness::prompt_catalog::ReviewFindingActionV1::FollowUp),
            severity: crate::harness::prompt_catalog::Severity::Low,
            kind: crate::harness::prompt_catalog::FindingKind::Overengineering,
            scope: None,
            location: "src/lib.rs:1".into(),
            evidence: vec!["verified-test-report-0".into()],
            repro: None,
            proposed_correction: "Remove the single-use wrapper.".into(),
        });
    assert!(validate_receipt(&value).is_ok());
    assert!(!is_approved_verdict(&value.verdict));
    assert!(!uses_current_prompt(&value));

    let root = tempfile::tempdir().unwrap();
    crate::artifacts::atomic_write_bytes(
        &root.path().join(RECEIPT_FILE),
        &serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let loaded = load_receipt(root.path(), &value.task_uid).unwrap().unwrap();
    assert_eq!(
        crate::harness::prompt_catalog::validate_prompt_manifest(&loaded.prompt).unwrap(),
        crate::harness::prompt_catalog::PromptRole::CodeReviewerV1
    );
    assert_eq!(loaded.verdict.findings[0].scope, None);
}

#[test]
fn persisted_review_receipt_round_trips_and_is_bound_to_its_subject() {
    let root = tempfile::tempdir().unwrap();
    let value = receipt();
    crate::artifacts::atomic_write_bytes(
        &root.path().join(RECEIPT_FILE),
        &serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let loaded = load_receipt(root.path(), &value.task_uid).unwrap().unwrap();
    assert_eq!(loaded.subject_hash, value.subject_hash);
    assert!(loaded.verdict.pass);

    let mut stale = loaded;
    stale.subject_hash = "f".repeat(64);
    assert!(validate_receipt(&stale).is_err());
}

#[test]
fn unchanged_failed_review_requires_manager_disposition_before_retry() {
    let mut failed = receipt();
    failed.subject_hash = "c".repeat(64);
    failed.verdict.subject_hash = failed.subject_hash.clone();
    failed.verdict.pass = false;

    assert!(same_code_failed_review_requires_manager_resolution(
        &failed,
        &failed.base_sha,
        &failed.head_sha,
        &failed.diff_hash,
        false,
    ));
    assert!(
        !same_code_failed_review_requires_manager_resolution(
            &failed,
            &failed.base_sha,
            &failed.head_sha,
            &failed.diff_hash,
            true,
        ),
        "an independently authorized contest arbitration may re-review the frozen code"
    );

    let mut repaired = failed.clone();
    repaired.head_sha = "d".repeat(40);
    assert!(!same_code_failed_review_requires_manager_resolution(
        &failed,
        &repaired.base_sha,
        &repaired.head_sha,
        &repaired.diff_hash,
        false,
    ));
}

#[test]
fn receipt_rejects_same_author_and_reviewer_session() {
    let mut value = receipt();
    value.reviewer_session_id = value.author_session_id.clone();
    assert!(validate_receipt(&value).is_err());

    let mut value = receipt();
    value.reviewer_run_id = value.author_run_id.clone();
    assert!(validate_receipt(&value).is_err());
}

#[test]
fn receipt_rejects_pass_with_findings_or_without_test_evidence() {
    let mut value = receipt();
    value
        .verdict
        .findings
        .push(crate::harness::prompt_catalog::ReviewFindingV1 {
            id: "finding-1".into(),
            requirement: "AC-1".into(),
            location_detail: Some(crate::harness::prompt_catalog::ReviewLocationV1 {
                file: "src/lib.rs".into(),
                line_start: Some(1),
                line_end: Some(1),
            }),
            rationale: "The required behavior is missing from the reviewed task.".into(),
            affected_requirement_ids: vec!["AC-1".into()],
            suggested_action: Some(crate::harness::prompt_catalog::ReviewFindingActionV1::FollowUp),
            severity: crate::harness::prompt_catalog::Severity::High,
            kind: crate::harness::prompt_catalog::FindingKind::Correctness,
            scope: Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking),
            location: "src/lib.rs:1".into(),
            evidence: vec!["The required behavior is missing".into()],
            repro: None,
            proposed_correction: "Implement the missing behavior".into(),
        });
    assert!(validate_receipt(&value).is_err());

    let mut value = receipt();
    value.verdict.untested.push("deployment".into());
    value.verdict.coverage[0].evidence.clear();
    assert!(validate_receipt(&value).is_err());
}

#[test]
fn passing_review_may_carry_nonblocking_findings_for_manager_follow_up() {
    let mut value = receipt();
    value.affected_paths = vec!["src/lib.rs".into()];
    value.verdict.examined_files = vec!["src/lib.rs".into()];
    value
        .verdict
        .findings
        .push(crate::harness::prompt_catalog::ReviewFindingV1 {
            id: "finding-1".into(),
            requirement: "AC-1".into(),
            location_detail: Some(crate::harness::prompt_catalog::ReviewLocationV1 {
                file: "src/lib.rs".into(),
                line_start: Some(1),
                line_end: Some(1),
            }),
            rationale: "A single-use abstraction adds unnecessary indirection.".into(),
            affected_requirement_ids: vec!["AC-1".into()],
            suggested_action: Some(crate::harness::prompt_catalog::ReviewFindingActionV1::FollowUp),
            severity: crate::harness::prompt_catalog::Severity::Low,
            kind: crate::harness::prompt_catalog::FindingKind::Overengineering,
            scope: Some(crate::harness::prompt_catalog::FindingScopeV1::NonBlocking),
            location: "src/lib.rs:1".into(),
            evidence: vec!["verified-test-report-0".into()],
            repro: None,
            proposed_correction: "Move the helper into the existing module.".into(),
        });
    assert!(validate_receipt(&value).is_ok());
    assert!(is_approved_verdict(&value.verdict));
    assert!(uses_current_prompt(&value));
}
