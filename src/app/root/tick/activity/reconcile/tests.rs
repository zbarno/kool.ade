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
/// Covers the tick reconciliation boundary: an active arbitration retains its
/// claim/evidence until the async result handler can retry a failure. A unit
/// test of the selector cannot prove this ordering across app activity. Uses a
/// temporary local Git project; no external services, normal fixture setup cost.
fn active_arbitration_claim_survives_inactive_work_reconciliation() {
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
    let markdown = crate::domain::ArtifactIdentity::preserve_markdown(
        "# CHG-021: Example\n\n**Status:** Draft\n",
        None,
        "CHG-021",
        "Example",
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .unwrap();
    let feature = crate::domain::ChangeMetadata::write_markdown(
        &markdown,
        &identity,
        crate::domain::ChangeStatus::Draft,
    )
    .unwrap();
    project
        .state
        .active_features
        .push(("CHG-021".into(), feature));

    let hash = "b".repeat(64);
    let mut card = crate::core::planning_work::Work::new(
        String::new(),
        "Independent arbitration: F-1".into(),
        "Assess the contested finding".into(),
        format!(
            "Snapshot hash: {hash}\nArbitration evidence: product-spec:reporting-contract\nArbitration evidence SHA-256: {}\nArbitration source quote: Empty results produce a valid file.\nArbitration rationale: The product specification directly requires a valid empty export.",
            "a".repeat(64)
        ),
    );
    card.kind = crate::core::planning_work::WorkKind::PlanReviewDisposition;
    card.status = crate::core::planning_work::WorkStatus::InProgress;
    card.feature_id = Some("CHG-021".into());
    card.key = format!("task:manager-plan-contest:{hash}:F-1");
    project.planning_work.push(card);
    project.activity.plan_review_feature = Some("CHG-021".into());
    project.save_planning_work().unwrap();

    reconcile_inactive_planning_work(&mut project);
    assert_eq!(
        project.planning_work[0].status,
        crate::core::planning_work::WorkStatus::InProgress
    );
    assert!(
        project.planning_work[0]
            .detail
            .contains("Arbitration evidence: product-spec:reporting-contract")
    );
    super::super::plan_review::lifecycle::mark_arbitration_failed(
        &mut project,
        "CHG-021",
        "asynchronous review failed",
    );
    assert_eq!(
        project.planning_work[0].status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
    assert!(crate::app::manager::next_plan_arbitration(&mut project).is_some());
    project.activity.plan_review_feature = None;
    reconcile_inactive_planning_work(&mut project);
    assert!(
        project.planning_work[0]
            .key
            .starts_with("task:manager-plan-repair:")
    );
    assert_eq!(
        project.planning_work[0].status,
        crate::core::planning_work::WorkStatus::Todo
    );
}
