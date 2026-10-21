use super::*;
use crate::core::planning_work::{CodeReviewFindingDispositionV1, Work};
use std::{path::Path, process::Command};

fn context() -> ManagerRepairContextV1 {
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
fn resolution_requires_fresh_review_of_the_new_current_head_and_subject() {
    let context = context();
    let new_head = "c".repeat(40);
    let new_subject = "d".repeat(64);
    assert!(review_matches_new_head(
        &context,
        &new_head,
        &new_subject,
        Some(&new_head),
        Some(&new_subject),
    ));
    assert!(!review_matches_new_head(
        &context,
        &context.reviewed_head_sha,
        &new_subject,
        Some(&context.reviewed_head_sha),
        Some(&new_subject),
    ));
    assert!(!review_matches_new_head(
        &context,
        &new_head,
        &new_subject,
        Some(&context.reviewed_head_sha),
        Some(&new_subject),
    ));
}

#[test]
fn repair_is_not_resolved_when_the_same_finding_returns_with_a_new_id() {
    let mut prior_finding = crate::harness::prompt_catalog::ReviewFindingV1 {
        id: "original-reviewer-id".into(),
        requirement: "AC-4".into(),
        location_detail: None,
        rationale: String::new(),
        affected_requirement_ids: Vec::new(),
        suggested_action: None,
        severity: crate::harness::prompt_catalog::Severity::High,
        kind: crate::harness::prompt_catalog::FindingKind::Correctness,
        scope: Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking),
        location: "src/worker.rs:23".into(),
        evidence: vec!["diff-1".into()],
        repro: Some("The worker accepts a stale task lease after restart.".into()),
        proposed_correction: "Reject stale leases.".into(),
    };
    let fingerprint = crate::core::implementation::finding_fingerprint(&prior_finding);
    prior_finding.location = "src/worker.rs:91".into();
    prior_finding.id = "reviewer-generated-new-id".into();
    let context = ManagerRepairContextV1 {
        finding_fingerprint: Some(fingerprint),
        ..context()
    };
    let finding = crate::harness::prompt_catalog::ReviewFindingV1 { ..prior_finding };
    let review = crate::core::implementation::ManagerCodeReviewFindings {
        task_uid: context.task_uid.clone(),
        subject_hash: "d".repeat(64),
        diff_hash: "e".repeat(64),
        head_sha: "f".repeat(40),
        prompt_hash: "a".repeat(64),
        reviewer_run_id: "reviewer-run".into(),
        reviewer_session_id: "reviewer-session".into(),
        pass: true,
        outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass),
        coverage: Vec::new(),
        untested: Vec::new(),
        examined_files: Vec::new(),
        tested_evidence_ids: Vec::new(),
        unexamined_areas: Vec::new(),
        findings: vec![finding],
    };
    assert!(!repair_finding_resolved(&context, &review));
}

#[test]
fn missing_review_receipt_moves_repair_work_to_needs_attention() {
    let temp = tempfile::tempdir().unwrap();
    init_git(temp.path());
    let mut project = crate::app::session::test_support::project_from(temp.path());
    let context = context();
    let mut work = Work::new(
        String::new(),
        "Code review repair".into(),
        "Repair the blocking finding".into(),
        "Assigned by Manager".into(),
    );
    work.uid = context.work_id.clone();
    work.kind = WorkKind::CodeReviewDisposition;
    work.status = WorkStatus::InProgress;
    work.parent_uid = Some(context.task_uid.clone());
    work.code_review_disposition = Some(CodeReviewFindingDispositionV1 {
        finding_id: context.finding_id.clone(),
        subject_hash: context.reviewed_subject_hash.clone(),
        scope: Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking),
        finding_fingerprint: context.finding_fingerprint.clone(),
        occurrence_count: 1,
        arbitration_attempts: 0,
        status: CodeReviewDispositionStatusV1::AssignedRepair,
        decision_reason: context.manager_reason.clone(),
        owner_work_id: context.work_id.clone(),
        evidence_ref: context.evidence_ref.clone(),
    });
    project.planning_work.push(work);

    mark_attention(
        &mut project,
        &context,
        "The worker completed without a validated independent review receipt for its current head.",
    );

    assert_eq!(project.planning_work[0].status, WorkStatus::NeedsAttention);
    assert!(
        project.planning_work[0]
            .detail
            .contains("without a validated")
    );
}

fn init_git(root: &Path) {
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git is installed");
        assert!(output.status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Fixture User"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(root.join("README.md"), "fixture\n").unwrap();
    git(&["add", "README.md"]);
    git(&["commit", "-qm", "fixture"]);
}
