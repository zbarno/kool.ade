use super::*;

#[test]
// Production boundary: a completed automatic Planner pass must resume an
// already approved task-generation continuation. Unit tests of the Planner
// and generation gates cannot prove their app-level handoff across worker
// completion. Dependencies: in-process app fixture and temporary local Git /
// planning stores; no external service. Setup cost: moderate (Planner plus task
// generation worker turns).
fn automatic_planner_completion_resumes_approved_task_generation() {
    use crate::core::planning::{MATERIAL_AXES, QuestionStore};

    let _shield = crate::core::gitops::test_support::shield("planner-generation-continuation");
    let (mut app, root, _) = setup();
    let context = egui::Context::default();
    frame(&mut app, &context, vec![]);
    click_text(&mut app, &context, "Review plan");
    click_text(&mut app, &context, "Confirm CHG-004 and prepare tasks");
    record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    app.approve_feature_from_board("CHG-004");
    assert!(app.feature_approved("CHG-004"));

    // The approved feature already has its persisted source and destination
    // branch choice, as it does when a Main Chat turn revises an existing plan.
    let Screen::Connected(project) = &mut app.screen else {
        panic!("project remains connected")
    };
    let mut workflow = project.state.workflow.clone();
    workflow.reviewed_specification = project.state.planning_contract().map(str::to_owned);
    workflow.feature_branch_targets.insert(
        "CHG-004".into(),
        crate::core::workflow::BranchTargets {
            source: "main".into(),
            destination: "main".into(),
        },
    );
    let (changes, checks, _) = crate::artifacts::task_docs::workflow_record_changes(
        &project.state.planning_store,
        &workflow,
    )
    .unwrap();
    let (_, revision) = project
        .state
        .planning_store
        .transaction_with_revision_and_record_revisions(
            &changes,
            Some(&project.state.baseline_planning_revision),
            &checks,
        )
        .unwrap();
    project.state.baseline_planning_revision = revision;
    project.state.workflow = workflow;

    let (feature_uid, plan_revision, repo, contract, work_key) = {
        let Screen::Connected(project) = &app.screen else {
            panic!("project remains connected")
        };
        let (_, markdown) = project.state.active_feature.as_ref().unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .unwrap()
            .unwrap();
        let (readiness, _) = QuestionStore::new(&project.state.planning_store)
            .load_readiness(&identity.uid)
            .unwrap()
            .unwrap();
        let work_key = project
            .planning_work
            .iter()
            .find(|work| work.kind == crate::core::planning_work::WorkKind::TaskGeneration)
            .map(|work| work.key.clone())
            .expect("approval creates a task-generation card");
        (
            identity.uid,
            readiness.plan_revision,
            project.state.repo_root.clone(),
            crate::core::workflow::feature_contract(markdown),
            work_key,
        )
    };
    // This is the continuation created by an approved Main Chat review. The
    // automatic Planner pass temporarily owns the app before generation can
    // resume.
    app.pending_feature_generation = Some((repo, "CHG-004".into(), contract, work_key));
    if let Screen::Connected(project) = &mut app.screen {
        project.activity.pending.clear();
        project.activity.last_update = Some(Instant::now());
    }

    let snapshot = {
        let Screen::Connected(project) = &app.screen else {
            panic!("project remains connected")
        };
        let (_, markdown) = project.state.active_feature.as_ref().unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .unwrap()
            .unwrap();
        crate::core::lifecycle::plan_snapshot::from_feature_with_author(
            &project.state.planning_store,
            "CHG-004",
            &identity,
            markdown,
            &crate::core::workflow::feature_contract(markdown),
        )
        .unwrap()
    };
    let planner_reply = serde_json::json!({
        "proposedQuestions": [],
        "draftChanges": [],
        "readiness": {
            "ready": true,
            "blockers": [],
            "evidence": [format!("feature:{feature_uid}")],
            "assessedAxes": MATERIAL_AXES,
            "acceptanceTests": snapshot.requirements.iter().map(|requirement| {
                serde_json::json!({
                    "requirementId": requirement.stable_id,
                    "testRef": "tests/approved-feature.rs"
                })
            }).collect::<Vec<_>>()
        }
    })
    .to_string();
    let (planner_harness, _) = harness(vec![planner_reply]);
    app.task_harness = Some(planner_harness);
    app.start_clarification_planner(&feature_uid, plan_revision);
    let (generation_harness, generated) = harness(vec![decomposer_graph(&app)]);
    app.task_harness = Some(generation_harness);

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        app.tick(0.016, &context);
        if matches!(&app.screen, Screen::Connected(project)
            if project.activity.clarification_planner.is_none())
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Planner completion did not resume approved task generation"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let continuation_state = match &app.screen {
        Screen::Connected(project) => format!(
            "turn={}, ready={}, approved={}, pending={}",
            project.active_turn.is_some(),
            project
                .state
                .workflow
                .ready(project.state.planning_contract()),
            crate::core::workflow::feature_approved(
                &project.state.planning_store,
                &project.state.workflow,
                "CHG-004",
            ),
            app.pending_feature_generation.is_some(),
        ),
        Screen::Welcome => "welcome screen".into(),
    };
    assert!(
        matches!(&app.screen, Screen::Connected(project) if project.active_turn.is_some()),
        "Planner result did not resume task generation ({continuation_state}): {:?}",
        app.chat_messages()
    );
    assert!(app.pending_feature_generation.is_none());
    assert!(finish(&mut app), "{:?}", app.chat_messages());
    assert_eq!(generated.lock().unwrap().len(), 1);
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    assert!(has_current_task_batch(project));
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
