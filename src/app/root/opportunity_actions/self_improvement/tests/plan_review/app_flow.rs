use super::*;

/// Production boundary: authenticated Pursue must flow through the app's current
/// user confirmation, isolated PlanReviewer dispatch, receipt persistence,
/// task generation, and Evolve projection to Ready. Store or state-machine tests
/// cannot prove this cross-module ordering. Uses a headless app, temporary
/// planning store, local fake `gh`, and deterministic in-process reviewers; no
/// network, Docker, or model service. Setup cost is one app fixture and two
/// short worker turns.
#[cfg(unix)]
#[test]
fn pursued_self_improvement_uses_current_app_plan_review_before_task_generation() {
    use std::os::unix::fs::PermissionsExt;

    let mut app = crate::app::root::board_tests::fixture();
    let opportunity_uid = {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        let remote = std::process::Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/zbarno/kool.ade",
            ])
            .current_dir(&project.state.repo_root)
            .output()
            .unwrap();
        assert!(remote.status.success());
        app_product_proposal(project)
    };
    let (planning, opportunity_revision) = match &mut app.screen {
        Screen::Connected(project) => {
            project
                .state
                .resync()
                .expect("Pursue proposal reloads from the planning store");
            project
                .state
                .bootstrap_missing()
                .expect("Pursue fixture has a bootstrapped planning packet");
            let planning = project.state.planning_store.clone();
            let (_, revision) = OpportunityStore::new(&planning)
                .load(&opportunity_uid)
                .unwrap();
            (planning, revision)
        }
        Screen::Welcome => panic!("fixture should remain connected"),
    };

    let temp = tempfile::tempdir().unwrap();
    let gh = temp.path().join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\n[ \"$1\" = api ] && [ \"$2\" = user ] || exit 2\nprintf '987654321\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let pursue_message = crate::app::root::self_improvement_test_support::decide_product_pursuit(
        &mut app,
        &opportunity_uid,
        opportunity_revision,
        &gh,
    );
    assert!(
        pursue_message
            .as_deref()
            .is_some_and(|message| message.contains("self-improvement run were added"))
    );

    let (feature_id, feature_uid, run_uid) = {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should remain connected")
        };
        let feature_id = project
            .state
            .active_feature
            .as_ref()
            .expect("Pursue creates a draft feature")
            .0
            .clone();
        let feature_uid =
            ArtifactIdentity::from_markdown(&project.state.active_feature.as_ref().unwrap().1)
                .unwrap()
                .unwrap()
                .uid;
        let decision = OpportunityStore::new(&planning)
            .decision_for_opportunity(&opportunity_uid)
            .unwrap()
            .expect("Pursue decision is persisted");
        let run_uid = SelfImprovementStore::run_uid_for_decision(&decision.uid).unwrap();
        let run = SelfImprovementStore::new(&planning).load(&run_uid).unwrap();
        assert_eq!(run.feature_uid, feature_uid);
        assert_eq!(run.phase, Phase::Planning);
        assert!(run.clone_uid.is_none());

        fixture::prepare_reviewable_plan(project, &feature_id);
        (feature_id, feature_uid, run_uid)
    };

    app.dispatch(crate::ui::ApplicationCommand::ChooseFeaturePlan {
        id: feature_id.clone(),
        plan_id: "B".into(),
    });
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    let comparison = project
        .state
        .workflow
        .plan_comparisons
        .get(&feature_id)
        .expect("the user's plan choice is persisted");
    assert_eq!(
        comparison.status,
        crate::core::workflow::PlanComparisonStatus::Adopted
    );
    assert_eq!(comparison.selected_plan.as_deref(), Some("B"));
    fixture::prepare_plan_review_snapshot(project, &feature_id);

    let action = app
        .available_feature_actions(None)
        .into_iter()
        .find(|action| action.id == feature_id)
        .expect("current self-improvement plan exposes its confirmation action");
    assert!(action.ready, "current plan readiness gates confirmation");
    assert!(!action.user_confirmed);
    assert!(!action.compare_plans, "the user adopted a saved plan");
    assert_eq!(
        action
            .plan_comparison
            .as_ref()
            .and_then(|comparison| comparison.selected_plan.as_deref()),
        Some("B")
    );
    let (active_turn, task_turns) = match &app.screen {
        Screen::Connected(project) => (
            project.active_turn.is_some(),
            project
                .task_turns
                .keys()
                .filter(|key| {
                    project
                        .state
                        .items
                        .iter()
                        .chain(&project.state.resolved_items)
                        .any(|item| {
                            item.conversation_key() == key.as_str()
                                && item.feature_id.as_deref() == Some(feature_id.as_str())
                        })
                })
                .cloned()
                .collect::<Vec<_>>(),
        ),
        Screen::Welcome => unreachable!(),
    };
    assert!(
        !active_turn && task_turns.is_empty(),
        "ordinary planning turns must finish before plan confirmation"
    );
    app.approve_feature_action_with_harness_for_test(
        &feature_id,
        Box::new(reviewer::PassingPlanReviewer),
    );

    let snapshot = {
        let Screen::Connected(project) = &app.screen else {
            panic!("fixture should remain connected")
        };
        let (_, markdown) = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == &feature_id)
            .unwrap();
        let identity = ArtifactIdentity::from_markdown(markdown)
            .unwrap()
            .expect("reviewed feature has stable identity");
        plan_snapshot::from_feature_with_author(
            &planning,
            &feature_id,
            &identity,
            markdown,
            &crate::core::workflow::feature_contract(markdown),
        )
        .unwrap()
    };
    let user_confirmation = plan_snapshot::user_confirmation_receipt(&planning, &snapshot).unwrap();
    assert!(
        user_confirmation.is_some(),
        "app did not persist user confirmation"
    );
    assert_eq!(
        snapshot.capability_id.as_deref(),
        Some("self-improvement-lifecycle")
    );
    assert!(
        plan_snapshot::has_current_user_confirmation(&planning, &snapshot).unwrap(),
        "user confirmation must still match the current product and approved plan"
    );

    let context = egui::Context::default();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        app.tick(0.016, &context);
        let reviewing = matches!(
            &app.screen,
            Screen::Connected(project) if project.activity.plan_review.is_some()
        );
        if !reviewing {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "plan reviewer finishes in budget"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let receipt = plan_snapshot::plan_review_receipt(&planning, &snapshot)
        .unwrap()
        .expect("the app reviewer receipt is persisted");
    assert_eq!(
        receipt.actor,
        crate::core::lifecycle::GateActor::IndependentReviewer
    );
    assert_eq!(
        receipt.prompt_role(),
        Some(crate::harness::prompt_catalog::PromptRole::PlanReviewer)
    );
    assert_ne!(receipt.reviewer_run_id, receipt.author_run_id);
    assert_ne!(receipt.reviewer_session_id, receipt.author_session_id);
    assert!(
        plan_snapshot::plan_review_verdict(&planning, &snapshot)
            .unwrap()
            .is_some_and(|verdict| verdict.pass)
    );

    let lifecycle = LifecycleStore::new(&planning);
    let feature = lifecycle.load_feature(&feature_uid).unwrap();
    assert_eq!(feature.phase, FeaturePhase::ReadyToDecompose);
    assert_eq!(
        feature.approved_plan_sha256.as_deref(),
        Some(snapshot.content_hash.as_str())
    );

    app.approve_feature_from_board(&feature_id);
    crate::app::root::self_improvement_test_support::synchronize_run(&mut app, &planning, &run_uid)
        .unwrap();
    let run = SelfImprovementStore::new(&planning).load(&run_uid).unwrap();
    assert_eq!(run.phase, Phase::Planning);
    assert!(run.clone_uid.is_none());
    assert!(matches!(
        &app.screen,
        Screen::Connected(project)
            if project.planning_work.iter().any(|work| {
                work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                    && work.feature_uid.as_deref() == Some(feature_uid.as_str())
                    && work.status == crate::core::planning_work::WorkStatus::Todo
            })
    ));

    task_generation::generate_and_sync(&mut app, &planning, &feature_id, &feature_uid, &run_uid);
}
