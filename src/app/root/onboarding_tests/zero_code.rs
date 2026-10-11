use super::super::HangingTurnHarness;
use super::support::*;
use crate::app::root::{KooladeApp, Screen};

#[test]
fn welcome_ui_creates_plans_joins_and_reopens_one_zero_code_workspace() {
    let _shield = crate::core::gitops::test_support::shield_with_synthetic_identity(
        "workspace-onboarding-ui",
    );
    let base = std::env::temp_dir().join(format!(
        "koolade_onboarding_ui_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home_a = base.join("operator-a");
    let home_b = base.join("operator-b");
    std::fs::create_dir_all(&home_a).unwrap();
    std::fs::create_dir_all(&home_b).unwrap();
    let _restore = StateHome::new();
    StateHome::set(&home_a);
    let ctx = egui::Context::default();
    let mut app = KooladeApp::default();

    type_into(&mut app, &ctx, "Project name", "Zero code workspace");
    click(&mut app, &ctx, "Create workspace");
    let Screen::Connected(project) = &app.screen else {
        let visible = visible_text(&frame(&mut app, &ctx, Vec::new()));
        panic!(
            "Create workspace should open a planning board; error: {:?}; visible: {visible:?}",
            app.conn_error
        )
    };
    let workspace_id = project.state.planning_store.project_id;
    assert!(project.state.repositories.repositories.is_empty());
    assert!(!project.state.repo_root.join(".koolade-packet").exists());
    assert!(!crate::ui::Surface::is_git_repo(&app));

    app.task_harness = Some(Box::new(HangingTurnHarness));
    click(&mut app, &ctx, "+ New Task");
    type_into(
        &mut app,
        &ctx,
        "Describe what you want to do",
        "Plan a zero-code workspace task through the board UI",
    );
    click(&mut app, &ctx, "Create Task");
    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    let work = project.planning_work.last().unwrap();
    let task_uid = work.uid.clone();
    assert_eq!(work.source_branch, None);
    assert_eq!(work.destination_branch, None);
    assert!(work.request.contains("zero-code workspace task"));
    wait_for_cancelled_turn(&mut app);

    let manager_a = crate::core::managed_workspace::WorkspaceManager::from_state_root();
    let workspace_a = manager_a.open(workspace_id).unwrap();
    let work_path = workspace_a.store.layout().work_record(&task_uid).unwrap();
    let work_relative = work_path
        .strip_prefix(&workspace_a.planning_repo)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    crate::core::gitops::commit(
        &workspace_a.planning_repo,
        "Kool.ad/e: save onboarding fixture task",
        &[work_relative],
    )
    .unwrap();
    let remote = base.join("remotes/planning.git");
    clone_bare_remote(&workspace_a.planning_repo, &remote);
    manager_a
        .set_planning_remote(workspace_id, &remote.to_string_lossy())
        .unwrap();

    StateHome::set(&home_b);
    app.screen = Screen::Welcome;
    app.conn_join_remote.clear();
    let join_url = url::Url::from_file_path(&remote).unwrap().to_string();
    type_into(
        &mut app,
        &ctx,
        "Planning Git remote URL or local checkout path",
        &join_url,
    );
    click(&mut app, &ctx, "Join workspace");
    let Screen::Connected(joined) = &app.screen else {
        panic!("Join workspace should open the shared planning board")
    };
    assert_eq!(joined.state.planning_store.project_id, workspace_id);
    assert_ne!(joined.state.repo_root, workspace_a.planning_repo);
    assert!(joined.state.repositories.repositories.is_empty());
    let joined_work = joined
        .planning_work
        .iter()
        .find(|work| work.uid == task_uid)
        .unwrap();
    assert!(joined_work.request.contains("zero-code workspace task"));

    app.screen = Screen::Welcome;
    app.conn_workspace_id = Some(workspace_id);
    click(&mut app, &ctx, "Open local workspace");
    let Screen::Connected(reopened) = &app.screen else {
        panic!("Open local workspace should reopen the joined planning board")
    };
    assert_eq!(reopened.state.planning_store.project_id, workspace_id);
    assert_eq!(
        reopened
            .planning_work
            .iter()
            .map(|work| &work.uid)
            .collect::<Vec<_>>(),
        vec![&task_uid]
    );
    drop(app);
    let _ = std::fs::remove_dir_all(base);
}
