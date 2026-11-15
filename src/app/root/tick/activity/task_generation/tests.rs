use super::*;

#[test]
fn task_generation_waits_for_independent_plan_approval() {
    let (mut app, fixture_root, _) = crate::app::root::feature_approval_test_support::setup();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let feature_uid = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == "CHG-004")
        .and_then(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
        })
        .expect("fixture feature has immutable identity")
        .uid;
    let mut work = crate::core::planning_work::Work::new(
        "task-generation:test-approved-plan".into(),
        "Generate tasks for the confirmed feature".into(),
        "Generate the approved implementation tasks.".into(),
        String::new(),
    );
    work.kind = WorkKind::TaskGeneration;
    work.feature_id = Some("CHG-004".into());
    work.feature_uid = Some(feature_uid);
    project.planning_work.push(work);

    assert!(next_approved(project.as_ref(), false).is_none());
    drop(app);
    crate::app::root::feature_approval_test_support::remove_fixture_root(&fixture_root);
}

#[test]
fn task_generation_rejects_stale_feature_identity_after_plan_approval() {
    let (mut app, fixture_root, _) = crate::app::root::feature_approval_test_support::setup();
    crate::app::root::feature_approval_test_support::record_test_plan_review_and_merge(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let mut work = approved_task_generation_work(project.as_ref());
    work.feature_uid = Some("00000000-0000-4000-8000-000000000099".into());
    project.planning_work.push(work);

    assert!(next_approved(project.as_ref(), false).is_none());
    drop(app);
    crate::app::root::feature_approval_test_support::remove_fixture_root(&fixture_root);
}

#[test]
fn task_generation_defers_while_manager_is_active() {
    let (mut app, fixture_root, _) = crate::app::root::feature_approval_test_support::setup();
    crate::app::root::feature_approval_test_support::record_test_plan_review_and_merge(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    project
        .planning_work
        .push(approved_task_generation_work(project.as_ref()));
    project.activity.manager = Some(crate::app::manager::Manager::completed_for_test(Err(
        "manager is still being reconciled".into(),
    )));

    assert!(next_approved(project.as_ref(), false).is_none());
    drop(app);
    crate::app::root::feature_approval_test_support::remove_fixture_root(&fixture_root);
}

#[test]
fn task_generation_waits_for_a_selected_provider_route() {
    let settings_path = std::env::temp_dir().join(format!(
        "koolade-task-generation-settings-{}.json",
        uuid::Uuid::new_v4().hyphenated()
    ));
    crate::persistence::harness_settings::with_settings_path(settings_path.clone(), || {
        let _shield = crate::core::gitops::test_support::shield("task-generation-route-gate");
        let (mut app, fixture_root, _) = crate::app::root::feature_approval_test_support::setup();
        crate::app::root::feature_approval_test_support::record_test_plan_review_and_merge(
            &mut app,
            "CHG-004",
            crate::core::lifecycle::GateDecision::Approved,
        );
        app.approve_feature_from_board("CHG-004");
        let generation_key = match &app.screen {
            crate::app::root::Screen::Connected(project) => project
                .planning_work
                .iter()
                .find(|work| work.kind == WorkKind::TaskGeneration)
                .map(|work| work.key.clone())
                .expect("approval creates task-generation work"),
            _ => panic!("project remains connected"),
        };
        crate::app::root::feature_approval_test_support::approve_test_git_intent(
            &mut app,
            &generation_key,
            "CHG-004",
        );
        let crate::app::root::Screen::Connected(project) = &app.screen else {
            unreachable!()
        };
        assert!(
            crate::app::root::planning_work::task_generation_has_approved_git_intents(
                project,
                "CHG-004",
                project
                    .planning_work
                    .iter()
                    .find(|work| work.key == generation_key)
                    .and_then(|work| work.feature_uid.as_deref()),
            )
        );
        assert!(next_approved(project, false).is_none());
        drop(app);
        crate::app::root::feature_approval_test_support::remove_fixture_root(&fixture_root);
    });
    let _ = std::fs::remove_file(settings_path);
}

fn approved_task_generation_work(
    project: &crate::app::session::Project,
) -> crate::core::planning_work::Work {
    let feature_uid = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == "CHG-004")
        .and_then(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
        })
        .expect("fixture feature has immutable identity")
        .uid;
    let mut work = crate::core::planning_work::Work::new(
        "task-generation:test-approved-plan".into(),
        "Generate tasks for the confirmed feature".into(),
        "Generate the approved implementation tasks.".into(),
        String::new(),
    );
    work.kind = WorkKind::TaskGeneration;
    work.feature_id = Some("CHG-004".into());
    work.feature_uid = Some(feature_uid);
    work
}
