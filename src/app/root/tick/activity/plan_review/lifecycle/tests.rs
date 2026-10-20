use super::*;
use crate::core::planning_work::{Work, WorkKind, WorkStatus};
use std::{path::Path, process::Command};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git is installed");
    assert!(output.status.success(), "git {args:?}");
}

#[test]
fn unresolved_arbitration_remains_selectable_until_bounded_escalation() {
    // Production boundary: persisted Manager contest cards pass through normal arbitration
    // selection, failure recovery, and bounded escalation. Predicate tests cannot prove the
    // Project work projection persists these lifecycle transitions. External dependencies: none.
    // Setup cost: one temporary Git project with a small persisted planning-work fixture.
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-q"]);
    git(temp.path(), &["config", "user.name", "Fixture User"]);
    git(
        temp.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    std::fs::write(temp.path().join("README.md"), "fixture\n").unwrap();
    git(temp.path(), &["add", "README.md"]);
    git(temp.path(), &["commit", "-qm", "fixture"]);
    let mut project = crate::app::session::test_support::project_from(temp.path());
    let hash = "b".repeat(64);
    let mut card = Work::new(
        String::new(),
        "Independent arbitration: F-1".into(),
        "Assess the contested finding".into(),
        format!(
            "Snapshot hash: {hash}\nArbitration evidence: product-spec:reporting-contract\nArbitration evidence SHA-256: {}\nArbitration source quote: Empty results produce a valid file.\nArbitration rationale: The product specification directly requires a valid empty export.",
            "a".repeat(64)
        ),
    );
    card.kind = WorkKind::PlanReviewDisposition;
    card.status = WorkStatus::NeedsAttention;
    card.feature_id = Some("CHG-021".into());
    card.key = format!("task:manager-plan-contest:{hash}:F-1");
    project.planning_work.push(card);
    project.save_planning_work().unwrap();

    let (feature_id, arbitration) =
        crate::app::manager::next_plan_arbitration(&mut project).unwrap();
    let verdict = crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: hash.clone(),
        pass: false,
        outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::BlockingFindings),
        coverage: Vec::new(),
        findings: vec![crate::harness::prompt_catalog::ReviewFindingV1 {
            id: "F-1".into(),
            requirement: "REQ-1".into(),
            severity: crate::harness::prompt_catalog::Severity::High,
            kind: crate::harness::prompt_catalog::FindingKind::Requirements,
            location: "requirement REQ-1".into(),
            evidence: vec!["plan:snapshot".into()],
            repro: Some("The issue remains after arbitration.".into()),
            proposed_correction: "Revise the plan to address the finding.".into(),
        }],
        untested: Vec::new(),
    };
    let receipt = crate::core::lifecycle::GateReceipt {
        receipt_uid: "receipt-1".into(),
        subject_uid: "snapshot".into(),
        subject_sha256: hash,
        actor: crate::core::lifecycle::GateActor::IndependentReviewer,
        decision: crate::core::lifecycle::GateDecision::ChangesRequested,
        recorded_at_unix_ms: 1,
        evidence_refs: Vec::new(),
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: Some("reviewer".into()),
        author_run_id: Some("author".into()),
        reviewer_session_id: Some("review-session".into()),
        author_session_id: Some("author-session".into()),
    };
    record_plan_arbitration_work(&mut project, &feature_id, &arbitration, &verdict, &receipt)
        .unwrap();
    assert_eq!(project.planning_work[0].status, WorkStatus::NeedsAttention);
    let (feature_id, arbitration) =
        crate::app::manager::next_plan_arbitration(&mut project).unwrap();
    let mut second_receipt = receipt;
    second_receipt.receipt_uid = "receipt-2".into();
    record_plan_arbitration_work(
        &mut project,
        &feature_id,
        &arbitration,
        &verdict,
        &second_receipt,
    )
    .unwrap();
    assert!(
        project.planning_work[0]
            .key
            .starts_with("task:manager-plan-repair:")
    );
    assert_eq!(project.planning_work[0].status, WorkStatus::Todo);
    assert!(!crate::app::manager::has_pending_plan_contest(
        &project, "CHG-021"
    ));
    assert!(crate::app::manager::has_pending_plan_repair(
        &project, "CHG-021"
    ));
    assert!(
        crate::app::manager::next_plan_arbitration(&mut project).is_none(),
        "an unresolved finding must be escalated after the bounded arbitration attempts"
    );

    let mut startup_card = Work::new(
        String::new(),
        "Independent arbitration: F-2".into(),
        "Assess the second contested finding".into(),
        format!(
            "Snapshot hash: {}\nArbitration evidence: product-spec:reporting-contract\nArbitration evidence SHA-256: {}\nArbitration source quote: Empty results produce a valid file.\nArbitration rationale: The product specification directly requires a valid empty export.",
            "c".repeat(64),
            "a".repeat(64)
        ),
    );
    startup_card.kind = WorkKind::PlanReviewDisposition;
    startup_card.status = WorkStatus::InProgress;
    startup_card.feature_id = Some("CHG-021".into());
    startup_card.key = format!("task:manager-plan-contest:{}:F-2", "c".repeat(64));
    project.planning_work.push(startup_card);
    project.save_planning_work().unwrap();
    mark_arbitration_failed(&mut project, "CHG-021", "review preparation failed");
    assert_eq!(project.planning_work[1].status, WorkStatus::NeedsAttention);
    assert!(crate::app::manager::next_plan_arbitration(&mut project).is_some());
    mark_arbitration_failed(&mut project, "CHG-021", "review preparation failed again");
    assert!(
        project.planning_work[1]
            .key
            .starts_with("task:manager-plan-repair:")
    );
}

#[path = "tests/sibling_cards.rs"]
mod sibling_cards;
