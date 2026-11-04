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

#[test]
fn concurrent_manager_proposals_for_one_revision_share_a_fenced_work_identity() {
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

    // Load two independent app snapshots before either writer commits.
    let first = crate::app::session::test_support::project_from(temp.path());
    let second = crate::app::session::test_support::project_from(temp.path());
    let revision = "c".repeat(64);
    let uid = plan_disposition_uid("CHG-021", &revision, "F-7");
    assert_ne!(uid, plan_disposition_uid("CHG-022", &revision, "F-7"));
    let make_work = |kind: &str, detail: &str| {
        let mut work = Work::new(
            String::new(),
            "Repair finding F-7".into(),
            "Repair the cited current finding".into(),
            detail.into(),
        );
        work.uid = uid.clone();
        work.key = format!("task:manager-plan-{kind}:{revision}:F-7");
        work.kind = WorkKind::PlanReviewDisposition;
        work.status = WorkStatus::Todo;
        work.feature_id = Some("CHG-021".into());
        work
    };
    let first_work = vec![make_work("repair", "first proposal")];
    let second_work = vec![make_work("follow-up", "duplicate proposal")];
    let expected_revision = first.state.baseline_planning_revision.clone();
    assert_eq!(expected_revision, second.state.baseline_planning_revision);
    let store = first.state.planning_store.clone();
    let first_store = store.clone();
    let second_store = second.state.planning_store.clone();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let first_barrier = barrier.clone();
    let first_expected_revision = expected_revision.clone();
    let first_writer = std::thread::spawn(move || {
        first_barrier.wait();
        crate::core::planning_work::save_expected(
            &first_store,
            &first_work,
            &first_expected_revision,
        )
    });
    let second_writer = std::thread::spawn(move || {
        barrier.wait();
        crate::core::planning_work::save_expected(&second_store, &second_work, &expected_revision)
    });
    let outcomes = [first_writer.join().unwrap(), second_writer.join().unwrap()];
    assert_eq!(
        outcomes.iter().filter(|result| result.is_ok()).count(),
        1,
        "only one concurrent proposal may commit against the shared revision"
    );
    let stored = crate::core::planning_work::load(&store).unwrap();
    assert_eq!(stored.len(), 1);
    assert!(matches!(
        stored[0].detail.as_str(),
        "first proposal" | "duplicate proposal"
    ));
}

// Production boundary: validated Manager dispositions are rechecked against persisted plan-review
// evidence and then saved through the revision-fenced local store. Storage-only tests cannot prove
// that both authorization and concurrent state changes remain effective at this boundary.
// Invariant: two proposals against one reviewed revision yield one durable disposition only.
// External dependencies: none; setup cost is a temporary local code repo, local store, and review
// receipt with no provider calls or Docker.
#[test]
fn concurrent_validated_manager_dispositions_create_one_durable_record() {
    let (mut app, root, _) = crate::app::root::feature_approval_test_support::setup();
    crate::app::root::feature_approval_test_support::record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Rejected,
    );
    let store = crate::app::root::feature_approval_test_support::planning_store(&app);
    drop(app);

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let writers = ["first", "second"].map(|label| {
        let root = root.clone();
        let store = store.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let mut project = crate::app::session::test_support::project_from_store(&root, &store);
            let reviews = crate::app::manager::catalog::reviews::current_review_evidence(&project);
            let review = reviews
                .iter()
                .find(|review| review.target.feature_id == "CHG-004")
                .expect("current persisted review evidence");
            let finding_id = review.target.finding_ids.first().unwrap().clone();
            let evidence_hashes = reviews
                .iter()
                .flat_map(|review| {
                    std::iter::once(&review.item).chain(review.supporting_items.iter())
                })
                .map(|item| {
                    (
                        item.id.clone(),
                        format!("{:x}", sha2::Sha256::digest(item.content.as_bytes())),
                    )
                })
                .collect::<std::collections::HashMap<_, _>>();
            let expected_revision = project.state.baseline_planning_revision.clone();
            let action = crate::harness::prompt_catalog::ManagerActionV1 {
                kind: crate::harness::prompt_catalog::ManagerActionKind::RepairPlan,
                target: format!("CHG-004#{finding_id}"),
                reason: format!(
                    "Concurrent {label} proposal requests repair of the current reviewed finding."
                ),
                evidence: vec![review.id.clone()],
                expected_revision: review.target.revision.clone(),
                contradiction: None,
            };
            let expected_uid =
                plan_disposition_uid("CHG-004", &review.target.revision, &finding_id);
            barrier.wait();
            let result = crate::app::manager::dispositions::persist_dispositions(
                &mut project,
                &[action],
                &evidence_hashes,
                &expected_revision,
                &std::collections::HashMap::new(),
            );
            (result, expected_uid)
        })
    });
    let outcomes = writers.map(|writer| writer.join().unwrap());
    assert_eq!(
        outcomes
            .iter()
            .filter(|(result, _)| result.as_ref().is_ok_and(|messages| messages.is_empty()))
            .count(),
        1,
        "exactly one concurrent disposition may commit without a stale or write-conflict result"
    );

    let stored = crate::core::planning_work::load(&store)
        .unwrap()
        .into_iter()
        .filter(|work| {
            work.kind == WorkKind::PlanReviewDisposition
                && work.feature_id.as_deref() == Some("CHG-004")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        stored.len(),
        1,
        "the local store contains one disposition; outcomes: {outcomes:?}"
    );
    assert_eq!(stored[0].uid, outcomes[0].1);
    std::fs::remove_dir_all(&store.root).unwrap();
    crate::app::root::feature_approval_test_support::remove_fixture_root(&root);
}
