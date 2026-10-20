use super::*;
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
fn contest_card_is_claimed_once_with_its_cited_evidence() {
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
    let mut work = Work::new(
        String::new(),
        "Independent arbitration: F-1".into(),
        "Assess the contested finding".into(),
        "Arbitration evidence: product-spec:reporting-contract\nArbitration evidence SHA-256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nArbitration source quote: Empty results produce a valid file.\nArbitration rationale: The product specification directly requires a valid empty export.".into(),
    );
    work.kind = WorkKind::PlanReviewDisposition;
    work.status = WorkStatus::NeedsAttention;
    work.feature_id = Some("CHG-021".into());
    work.key = format!("task:manager-plan-contest:{}:F-1", "b".repeat(64));
    project.planning_work.push(work);
    project.save_planning_work().unwrap();

    let (feature, arbitration) = next_plan_arbitration(&mut project).unwrap();
    assert_eq!(feature, "CHG-021");
    assert_eq!(arbitration.finding_id, "F-1");
    assert_eq!(arbitration.evidence_id, "product-spec:reporting-contract");
    assert_eq!(
        arbitration.source_quote,
        "Empty results produce a valid file."
    );
    assert_eq!(
        project.planning_work[0].status,
        WorkStatus::InProgress,
        "the durable claim must precede worker startup"
    );
    assert!(
        next_plan_arbitration(&mut project).is_none(),
        "a claimed arbitration must not start twice"
    );
}

#[test]
fn failed_arbitration_is_retryable_twice_then_stays_blocked() {
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
    let mut work = Work::new(
        String::new(),
        "Independent arbitration: F-1".into(),
        "Assess the contested finding".into(),
        format!(
            "Snapshot hash: {}\nArbitration evidence: product-spec:reporting-contract\nArbitration evidence SHA-256: {}\nArbitration source quote: Empty results produce a valid file.\nArbitration rationale: The product specification directly requires a valid empty export.",
            "b".repeat(64),
            "a".repeat(64)
        ),
    );
    work.kind = WorkKind::PlanReviewDisposition;
    work.status = WorkStatus::NeedsAttention;
    work.feature_id = Some("CHG-021".into());
    work.key = format!("task:manager-plan-contest:{}:F-1", "b".repeat(64));
    project.planning_work.push(work);
    project.save_planning_work().unwrap();

    for _ in 0..2 {
        assert!(next_plan_arbitration(&mut project).is_some());
        let work = &mut project.planning_work[0];
        work.status = WorkStatus::NeedsAttention;
        work.detail
            .push_str("\nArbitration attempt failed: reviewer unavailable");
        project.save_planning_work().unwrap();
    }
    assert!(next_plan_arbitration(&mut project).is_none());
    assert!(has_pending_plan_contest(&project, "CHG-021"));
}
