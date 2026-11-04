use super::*;

#[test]
fn changed_contract_stops_authorized_continuation_and_manager_uses_current_state() {
    let _shield = crate::core::gitops::test_support::shield("feature-approval-continuation");
    let (mut app, root, review) = setup();
    let path = published_spec_path(&app, &root);
    let (h, _) = harness(vec![review.to_string()]);
    app.task_harness = Some(h);
    record_test_plan_author(&mut app, "CHG-004");
    app.approve_feature_only("CHG-004");
    record_test_plan_review_and_merge(
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
    approve_test_git_intent(&mut app, &task_generation_key, "CHG-004");
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
    let changed = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Persist saved searches", "Publish saved searches");
    std::fs::write(&path, changed).unwrap();
    commit_published_spec(&root);
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
    if let Screen::Connected(project) = &mut app.screen {
        let body = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == "CHG-004")
            .unwrap()
            .1
            .clone();
        readiness::seed_test_readiness(&project.state.planning_store, "CHG-004", &body);
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    assert_eq!(
        app.feature_actions(None)[0].label(),
        "Confirm plan and allow publication for CHG-004"
    );
    drop(app);
    remove_fixture_root(&root);
}

#[test]
fn stale_display_cannot_approve_changed_contract_or_overwrite_other_approval() {
    let _shield = crate::core::gitops::test_support::shield("feature-approval-drift");
    let (mut app, root, _) = setup();
    ensure_test_plan_adopted(&mut app, "CHG-004");
    record_test_plan_author(&mut app, "CHG-004");
    let displayed = match &app.screen {
        Screen::Connected(project) => project
            .state
            .active_feature
            .as_ref()
            .expect("feature remains the displayed planning focus")
            .1
            .clone(),
        Screen::Welcome => panic!("fixture remains connected"),
    };
    let changed = displayed.replace("Persist saved searches", "Publish saved searches");
    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    let store = project.state.planning_store.clone();
    let proposal = crate::core::apply::private_document_draft_changes(
        &store,
        &[("feature:CHG-004".into(), changed)],
    )
    .unwrap();
    store.transaction(&proposal, None).unwrap();
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
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    p.state
        .workflow
        .approved_features
        .insert("CHG-009".into(), "Other contract".into());
    let approval = workflow::approve_feature_if_current(
        &p.state.planning_store,
        &mut p.state.workflow,
        "CHG-004",
        Some(&workflow::feature_contract(
            &p.state.active_feature.as_ref().unwrap().1,
        )),
    );
    assert!(
        approval.is_err(),
        "stale feature contract must remain blocked"
    );
    assert_eq!(
        p.state.workflow.approved_features["CHG-009"],
        "Other contract"
    );
    drop(app);
    remove_fixture_root(&root);
}

#[test]
fn explicit_plan_confirmation_persists_snapshot_and_blocks_without_independent_review() {
    let _shield = crate::core::gitops::test_support::shield("plan-snapshot-approval");
    let (mut app, root, _) = setup();
    ensure_test_plan_adopted(&mut app, "CHG-004");
    record_test_plan_author(&mut app, "CHG-004");
    let store_root = private_store_root(&app);
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
    record_test_plan_review_and_merge(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    app.approve_feature_from_board("CHG-004");
    let markdown = {
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
        let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
            &project.state.planning_store,
            "CHG-004",
            &identity,
            markdown,
            &contract,
        )
        .unwrap();
        let path = store_root.join(format!("state/lifecycle_plans/{}.json", snapshot.uid));
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
        markdown.clone()
    };

    let changed = markdown.replace("Persist saved searches", "Publish saved searches");
    let spec_path = published_spec_path(&app, &root);
    std::fs::write(spec_path, changed).unwrap();
    commit_published_spec(&root);
    let Screen::Connected(project) = &mut app.screen else {
        panic!("project remains connected")
    };
    project.state = crate::core::state::PlannerState::load(&root).unwrap();
    assert!(!crate::core::workflow::feature_approved(
        &project.state.planning_store,
        &project.state.workflow,
        "CHG-004"
    ));
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

fn published_spec_path(app: &KooladeApp, root: &std::path::Path) -> std::path::PathBuf {
    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    let (_, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == "CHG-004")
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    root.join("docs/koolade/features")
        .join(format!("{}.md", identity.uid))
}

fn commit_published_spec(root: &std::path::Path) {
    assert!(
        std::process::Command::new("git")
            .args(["add", "docs/koolade/features"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .args([
                "commit",
                "-q",
                "-m",
                "Update synthetic published test feature"
            ])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
}
