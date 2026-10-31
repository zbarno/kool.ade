use super::*;

#[path = "readiness/confirmation_card.rs"]
mod confirmation_card;

pub(super) fn seed_test_readiness(
    store: &crate::artifacts::planning_store::PlanningStore,
    feature_id: &str,
    markdown: &str,
) {
    use crate::core::planning::{
        AcceptanceTestTrace, QuestionStore, ReadinessAxis, ReadinessChecklistV1,
    };
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    let contract = crate::core::workflow::feature_contract(markdown);
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        store, feature_id, &identity, markdown, &contract,
    )
    .unwrap();
    let axes = vec![
        ReadinessAxis::Who,
        ReadinessAxis::What,
        ReadinessAxis::When,
        ReadinessAxis::Where,
        ReadinessAxis::Why,
        ReadinessAxis::How,
        ReadinessAxis::OutOfScope,
        ReadinessAxis::CriticalPath,
        ReadinessAxis::SystemBoundaries,
        ReadinessAxis::FailureBehavior,
        ReadinessAxis::DataOwnership,
        ReadinessAxis::Authorization,
        ReadinessAxis::MeasurableSuccess,
        ReadinessAxis::TestCases,
    ];
    let acceptance_tests = snapshot
        .requirements
        .iter()
        .map(|requirement| AcceptanceTestTrace {
            requirement_id: requirement.stable_id.clone(),
            test_ref: "tests/feature.rs".into(),
        })
        .collect::<Vec<_>>();
    let mut checklist = ReadinessChecklistV1 {
        feature_uid: identity.uid.clone(),
        plan_revision: 1,
        spec_revision: Some(snapshot.spec_revision),
        required_requirement_ids: Some(
            snapshot
                .requirements
                .iter()
                .map(|requirement| requirement.stable_id.clone())
                .collect(),
        ),
        assessed_axes: axes,
        acceptance_tests,
        evidence_refs: vec!["test-fixture:approved-plan-readiness".into()],
        unresolved_blocker_ids: Vec::new(),
        blocker_notes: Vec::new(),
        ready: true,
        stalled: false,
        no_progress_count: 0,
        progress_fingerprint: None,
        revision: 0,
    };
    let questions = QuestionStore::new(store);
    let expected_revision =
        if let Some((previous, revision)) = questions.load_readiness(&identity.uid).unwrap() {
            checklist.revision = previous.revision.saturating_add(1);
            revision
        } else {
            0
        };
    questions
        .save_readiness(&checklist, expected_revision)
        .unwrap();
}

pub(super) fn current_snapshot(
    app: &KooladeApp,
    id: &str,
) -> crate::core::lifecycle::PlanSnapshotV1 {
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    let (_, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(feature_id, _)| feature_id == id)
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        id,
        &identity,
        markdown,
        &crate::core::workflow::feature_contract(markdown),
    )
    .unwrap()
}

#[path = "readiness/current_gate.rs"]
mod current_gate;

#[test]
fn readiness_binding_survives_store_restart_and_allows_current_confirmation() {
    let _shield = crate::core::gitops::test_support::shield("approval-readiness-restart");
    let (mut app, root, _) = setup();
    let snapshot = current_snapshot(&app, "CHG-004");
    let reopened = crate::artifacts::planning_store::PlanningStore::new(
        match &app.screen {
            Screen::Connected(project) => project.state.planning_store.project_id,
            Screen::Welcome => panic!(),
        },
        root.clone(),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let (readiness, _) = crate::core::planning::QuestionStore::new(&reopened)
        .load_readiness(&snapshot.feature_uid)
        .unwrap()
        .unwrap();
    assert_eq!(
        readiness.spec_revision.as_deref(),
        Some(snapshot.spec_revision.as_str())
    );
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    app.approve_feature_only("CHG-004");
    let Screen::Connected(project) = &app.screen else {
        panic!()
    };
    assert!(
        crate::core::lifecycle::plan_snapshot::has_user_confirmation(
            &project.state.planning_store,
            &snapshot
        )
        .unwrap()
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn confirmation_fails_closed_for_missing_readiness_binding() {
    let _shield = crate::core::gitops::test_support::shield("approval-readiness-missing");
    let (mut app, root, _) = setup();
    let snapshot = current_snapshot(&app, "CHG-004");
    std::fs::remove_file(root.join(format!("state/readiness/{}.json", snapshot.feature_uid)))
        .unwrap();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    app.approve_feature_only("CHG-004");
    let Screen::Connected(project) = &app.screen else {
        panic!()
    };
    assert!(
        !crate::core::lifecycle::plan_snapshot::has_user_confirmation(
            &project.state.planning_store,
            &snapshot
        )
        .unwrap()
    );
    assert!(
        app.feature_actions(None).is_empty(),
        "missing readiness evidence must not expose a confirmation card"
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn confirmation_fails_closed_for_stale_spec_and_newer_question_revisions() {
    let _shield = crate::core::gitops::test_support::shield("approval-readiness-stale");
    let (mut app, root, _) = setup();
    let snapshot = current_snapshot(&app, "CHG-004");
    let Screen::Connected(project) = &mut app.screen else {
        panic!()
    };
    let questions = crate::core::planning::QuestionStore::new(&project.state.planning_store);
    let (mut readiness, revision) = questions
        .load_readiness(&snapshot.feature_uid)
        .unwrap()
        .unwrap();
    readiness.spec_revision = Some("0".repeat(64));
    readiness.revision += 1;
    questions.save_readiness(&readiness, revision).unwrap();
    project.state = crate::core::state::PlannerState::load(&root).unwrap();
    app.approve_feature_only("CHG-004");
    let Screen::Connected(project) = &app.screen else {
        panic!()
    };
    assert!(
        !crate::core::lifecycle::plan_snapshot::has_user_confirmation(
            &project.state.planning_store,
            &snapshot
        )
        .unwrap()
    );
    assert!(
        app.feature_actions(None).is_empty(),
        "no confirmation action is available for stale readiness evidence"
    );
    let Screen::Connected(project) = &mut app.screen else {
        panic!()
    };
    let questions = crate::core::planning::QuestionStore::new(&project.state.planning_store);
    let (mut readiness, revision) = questions
        .load_readiness(&snapshot.feature_uid)
        .unwrap()
        .unwrap();
    readiness.spec_revision = Some(snapshot.spec_revision.clone());
    readiness.revision += 1;
    questions.save_readiness(&readiness, revision).unwrap();
    let question = crate::core::planning::propose_questions(
        vec![crate::core::planning::QuestionProposal {
            feature_uid: snapshot.feature_uid.clone(),
            plan_revision: readiness.plan_revision + 1,
            decision_subject: "current requirement scope".into(),
            decision_domain: "scope".into(),
            origin_requirement_ids: snapshot
                .requirements
                .first()
                .map(|requirement| vec![requirement.stable_id.clone()])
                .unwrap_or_default(),
            category: crate::core::planning::QuestionCategory::Scope,
            priority: crate::core::planning::QuestionPriority::Blocking,
            question: "Should the feature include this behavior?".into(),
            reason: "It changes acceptance scope.".into(),
            options: vec![crate::core::planning::QuestionOption {
                id: "include".into(),
                label: "Include it".into(),
                tradeoffs: "Larger scope.".into(),
            }],
            recommended_option_id: None,
            no_answer_impact: "Scope remains unresolved.".into(),
            evidence_refs: vec![],
            assumption_if_deferred: None,
            depends_on_question_ids: vec![],
        }],
        &[],
    )
    .unwrap()
    .remove(0);
    questions.save(&question, 0).unwrap();
    project.state.baseline_planning_revision = project.state.planning_store.revision().unwrap();
    project.state = crate::core::state::PlannerState::load(&root).unwrap();
    app.approve_feature_only("CHG-004");
    let Screen::Connected(project) = &app.screen else {
        panic!()
    };
    assert!(
        !crate::core::lifecycle::plan_snapshot::has_user_confirmation(
            &project.state.planning_store,
            &snapshot
        )
        .unwrap()
    );
    assert!(
        app.feature_actions(None).is_empty(),
        "a newer clarification record keeps the old candidate unavailable"
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
