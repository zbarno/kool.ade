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
