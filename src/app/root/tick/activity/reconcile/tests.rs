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

#[test]
/// Covers restart recovery for a persisted code-review arbitration claim: the
/// runtime worker is absent after restart, so the contested finding returns to
/// the retry selector without changing its frozen subject or attempt count.
/// Uses a temporary local Git project and persisted planning work only.
fn interrupted_code_review_arbitration_returns_to_needs_attention() {
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
    let mut work = crate::core::planning_work::Work::new(
        "task:code-review-disposition:task-1:F-1".into(),
        "Independent arbitration: F-1".into(),
        "Resolve the contested review finding".into(),
        "Arbitration attempt 1 dispatched on the frozen subject".into(),
    );
    work.kind = crate::core::planning_work::WorkKind::CodeReviewDisposition;
    work.status = crate::core::planning_work::WorkStatus::InProgress;
    work.parent_uid = Some("00000000-0000-4000-8000-000000000001".into());
    work.code_review_disposition =
        Some(crate::core::planning_work::CodeReviewFindingDispositionV1 {
            finding_id: "F-1".into(),
            subject_hash: "a".repeat(64),
            scope: Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking),
            finding_fingerprint: Some("b".repeat(64)),
            occurrence_count: 1,
            arbitration_attempts: 1,
            status: crate::core::planning_work::CodeReviewDispositionStatusV1::Contested,
            decision_reason: "The evidence is materially disputed.".into(),
            owner_work_id: work.uid.clone(),
            evidence_ref: format!("code-review-finding:{}", "c".repeat(24)),
        });
    let uid = work.uid.clone();
    project.planning_work.push(work);
    project.save_planning_work().unwrap();

    // Model an application restart: load a fresh project and reconstruct its
    // durable planning work from the same planning store. Runtime activity,
    // including the arbitration worker, starts empty.
    let mut restarted_project = crate::app::session::test_support::project_from(temp.path());
    restarted_project.planning_work =
        crate::core::planning_work::load(&restarted_project.state.planning_store).unwrap();
    reconcile_inactive_planning_work(&mut restarted_project);

    let recovered = &restarted_project.planning_work[0];
    assert_eq!(recovered.uid, uid);
    assert_eq!(
        recovered.status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
    let disposition = recovered.code_review_disposition.as_ref().unwrap();
    assert_eq!(
        disposition.status,
        crate::core::planning_work::CodeReviewDispositionStatusV1::Contested
    );
    assert_eq!(disposition.arbitration_attempts, 1);
    assert_eq!(disposition.subject_hash, "a".repeat(64));
    assert!(
        recovered
            .detail
            .contains("Recovered interrupted independent code-review arbitration")
    );
}
