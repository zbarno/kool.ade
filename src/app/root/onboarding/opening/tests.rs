use super::*;

#[test]
fn failed_session_save_does_not_start_workspace_loader() {
    let mut app = KooladeApp::default();
    app.onboarding.error = Some("storage unavailable".into());
    app.begin_onboarding_open(
        uuid::Uuid::new_v4(),
        Some(("idea".into(), "saved-work".into())),
    );
    assert!(app.onboarding_open.is_none());
    assert!(
        app.conn_error
            .as_deref()
            .unwrap()
            .contains("could not be saved")
    );
    assert!(app.onboarding.pending_interview.is_some());
}

#[test]
fn explicit_existing_project_open_can_continue_with_unsaved_navigation_state() {
    let temp = tempfile::tempdir().unwrap();
    let _state_home = crate::test_support::KooladeHomeGuard::set(temp.path());
    let mut app = KooladeApp::default();
    app.onboarding.step = welcome::onboarding::Step::ExistingProject;
    app.onboarding.existing = Some(crate::core::managed_workspace::ExistingProject {
        folder: temp.path().join("code"),
        planning_repo: Some(temp.path().join("planning")),
        workspace_id: uuid::Uuid::new_v4(),
        repository_id: "synthetic-code".into(),
        title: "Synthetic project".into(),
        create_workspace: false,
    });
    app.onboarding.error = Some("saved progress is unavailable".into());
    let id = uuid::Uuid::new_v4();

    app.begin_existing_onboarding_open(id);

    assert!(app.onboarding_open.is_some());
    assert_eq!(app.onboarding.opening_workspace_id, Some(id));
    let job = app.onboarding_open.take().unwrap();
    let _ = job.result.join().unwrap();
}

#[test]
fn unsaved_navigation_state_does_not_bypass_new_workspace_creation_guard() {
    let temp = tempfile::tempdir().unwrap();
    let _state_home = crate::test_support::KooladeHomeGuard::set(temp.path());
    let mut app = KooladeApp::default();
    app.onboarding.step = welcome::onboarding::Step::ExistingProject;
    app.onboarding.existing = Some(crate::core::managed_workspace::ExistingProject {
        folder: temp.path().join("code"),
        planning_repo: None,
        workspace_id: uuid::Uuid::new_v4(),
        repository_id: "synthetic-code".into(),
        title: "Synthetic project".into(),
        create_workspace: true,
    });
    app.onboarding.error = Some("saved progress is unavailable".into());

    app.begin_existing_onboarding_open(uuid::Uuid::new_v4());

    assert!(app.onboarding_open.is_none());
    assert!(
        app.conn_error
            .as_deref()
            .unwrap()
            .contains("could not be saved")
    );
}

#[test]
fn stale_loader_result_cannot_open_another_request() {
    // Controller result fence only: no disk, Git or provider calls are needed.
    let mut app = KooladeApp::default();
    let requested = uuid::Uuid::new_v4();
    app.onboarding.opening_workspace_id = Some(requested);
    let result = std::thread::spawn(|| Err(crate::error::AppError::Other("stale error".into())));
    while !result.is_finished() {
        std::thread::yield_now();
    }
    app.onboarding_open = Some(OpenJob {
        id: uuid::Uuid::new_v4(),
        result,
        interview: None,
    });
    app.poll_onboarding_open();
    assert_eq!(app.onboarding.opening_workspace_id, Some(requested));
    assert!(app.conn_error.is_none());
    assert!(matches!(app.screen, Screen::Welcome));
}
