use super::*;

#[test]
fn changed_contract_stops_authorized_continuation_and_manager_uses_current_state() {
    let _shield = crate::core::gitops::test_support::shield("feature-approval-continuation");
    let (mut app, root, review) = setup();
    let (h, _) = harness(vec![review.to_string()]);
    app.task_harness = Some(h);
    record_test_plan_author(&mut app, "CHG-004");
    app.approve_feature_only("CHG-004");
    record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    app.approve_feature_from_board("CHG-004");
    let task_generation_key = match &app.screen {
        Screen::Connected(project) => project
            .planning_work
            .iter()
            .find(|work| work.kind == crate::core::planning_work::WorkKind::TaskGeneration)
            .map(|work| work.key.clone())
            .expect("reviewed approval creates a task-generation card"),
        Screen::Welcome => panic!("project remains connected"),
    };
    app.start_task_generation(&task_generation_key, "CHG-004");
    assert!(finish(&mut app));
    assert!(
        !app.chat_messages()
            .last()
            .unwrap()
            .text
            .contains("Would you like to proceed")
    );
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    let prompt = crate::app::manager::Manager::prompt_body(p.as_ref(), &[]);
    assert!(prompt.contains("CHG-004: approved for current contract; do not ask again"));
    assert!(prompt.contains("Persist saved searches and restore them after restarting."));
    let path =
        root.join(".koolade-packet/planning/changes/CHG-004-saved-searches/specification.md");
    let changed = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Persist saved searches", "Publish saved searches");
    std::fs::write(&path, changed).unwrap();
    p.state = crate::core::state::PlannerState::load(&root).unwrap();
    app.continue_feature_generation(true);
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    assert!(p.active_turn.is_none());
    assert!(app.pending_feature_generation.is_none());
    assert!(!app.feature_approved("CHG-004"));
    assert!(
        app.chat_messages()
            .last()
            .unwrap()
            .text
            .contains("contract or planning focus changed")
    );
    assert_eq!(
        app.feature_actions(None)[0].label(),
        "Approve CHG-004 and prepare tasks"
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_display_cannot_approve_changed_contract_or_overwrite_other_approval() {
    let _shield = crate::core::gitops::test_support::shield("feature-approval-drift");
    let (mut app, root, _) = setup();
    let path =
        root.join(".koolade-packet/planning/changes/CHG-004-saved-searches/specification.md");
    let original = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        original.replace("Persist saved searches", "Publish saved searches"),
    )
    .unwrap();
    app.approve_and_prepare_feature("CHG-004");
    assert!(!app.feature_approved("CHG-004"));
    assert!(
        app.chat_messages()
            .last()
            .unwrap()
            .text
            .contains("changed since it was displayed")
    );
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    assert!(p.active_turn.is_none());
    std::fs::write(&path, original).unwrap();
    let other_directory = root.join(".koolade-packet/planning/changes/CHG-009-other-feature");
    std::fs::create_dir_all(&other_directory).unwrap();
    let other_path = other_directory.join("specification.md");
    let other_spec = crate::artifacts::product_docs::identity::preserve_feature_identity(
        &other_path,
        "CHG-009",
        "# CHG-009: Other feature\n\n## Intent\n\nA separate approved feature.\n",
    )
    .unwrap();
    std::fs::write(other_path, other_spec).unwrap();
    let mut saved = crate::artifacts::task_docs::load_workflow(&root).unwrap();
    saved
        .approved_features
        .insert("CHG-009".into(), "Other contract".into());
    crate::artifacts::task_docs::save_workflow(&root, &saved).unwrap();
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    workflow::approve_feature_if_current(
        &root,
        &mut p.state.workflow,
        "CHG-004",
        Some(&workflow::feature_contract(
            &p.state.active_feature.as_ref().unwrap().1,
        )),
    )
    .unwrap();
    assert_eq!(
        p.state.workflow.approved_features["CHG-009"],
        "Other contract"
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_plan_confirmation_persists_snapshot_and_blocks_without_independent_review() {
    let _shield = crate::core::gitops::test_support::shield("plan-snapshot-approval");
    let (mut app, root, _) = setup();
    app.approve_feature_from_board("CHG-004");
    assert!(!app.feature_approved("CHG-004"));
    let blocked_status = match &app.screen {
        Screen::Connected(project) => project
            .planning_work
            .iter()
            .find(|work| work.kind == crate::core::planning_work::WorkKind::TaskGeneration)
            .map(|work| (work.status, work.detail.clone()))
            .expect("confirmation creates a task-generation card"),
        Screen::Welcome => panic!("project remains connected"),
    };
    assert_eq!(
        blocked_status.0,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
    assert!(blocked_status.1.contains("independent PlanReviewer"));
    record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    app.approve_feature_from_board("CHG-004");
    let Screen::Connected(project) = &app.screen else {
        panic!()
    };
    assert!(crate::core::workflow::feature_approved(
        &project.state.planning_store,
        &project.state.workflow,
        "CHG-004"
    ));
    let (_, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == "CHG-004")
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    let contract = workflow::feature_contract(markdown);
    let snapshot = crate::core::lifecycle::PlanSnapshotV1::from_feature(
        "CHG-004", &identity, markdown, &contract,
    )
    .unwrap();
    let path = root.join(format!(
        ".koolade-packet/state/lifecycle_plans/{}.json",
        snapshot.uid
    ));
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(persisted["data"]["contentHash"], snapshot.content_hash);
    assert_eq!(
        persisted["data"]["approvalContractSha256"],
        snapshot.approval_contract_sha256
    );
    let task_generation = project
        .planning_work
        .iter()
        .find(|work| {
            work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                && work.feature_uid.as_deref() == Some(identity.uid.as_str())
        })
        .unwrap();
    assert_eq!(
        task_generation.status,
        crate::core::planning_work::WorkStatus::Todo
    );
    assert!(task_generation.detail.contains("PlanReviewer approved"));
    assert!(project.active_turn.is_none());

    let changed = markdown.replace("Persist saved searches", "Publish saved searches");
    let spec_path =
        root.join(".koolade-packet/planning/changes/CHG-004-saved-searches/specification.md");
    std::fs::write(spec_path, changed).unwrap();
    assert!(!crate::core::workflow::feature_approved(
        &project.state.planning_store,
        &project.state.workflow,
        "CHG-004"
    ));
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
