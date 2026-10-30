use super::support::*;
use crate::{
    app::root::{KooladeApp, Screen},
    core::managed_workspace::WorkspaceManager,
};

#[test]
fn real_connections_record_recency_but_listing_and_failed_opens_do_not() {
    // Boundary: app connection -> managed Git workspace -> private persisted MRU.
    // Storage-only unit tests cannot establish that failed opens do not record
    // a visit. Requires local Git, no Docker/network/provider; two small repos.
    let _shield =
        crate::core::gitops::test_support::shield_with_synthetic_identity("welcome-recency");
    let base = std::env::temp_dir().join(format!("koolade-recent-{}", uuid::Uuid::new_v4()));
    let mut home = StateHome::new();
    home.set(&base);
    let manager = WorkspaceManager::from_state_root();
    let first = manager.create("Alpha workspace").unwrap();
    let second = manager.create("Beta workspace").unwrap();
    let mut app = KooladeApp::default();
    assert!(manager.last_opened_dates().unwrap().is_empty());
    app.conn_workspace_id = Some(second.id);
    app.submit_open_local_workspace();
    assert!(matches!(&app.screen, Screen::Connected(project)
        if project.state.planning_store.project_id == second.id));
    let dates = manager.last_opened_dates().unwrap();
    assert!(dates.contains_key(&second.id));
    assert!(!dates.contains_key(&first.id));
    let mut listed = manager.list_local().unwrap();
    manager.sort_by_recent_open(&mut listed).unwrap();
    assert_eq!(listed[0].id, second.id);
    assert_eq!(manager.last_opened_dates().unwrap(), dates);
    app.screen = Screen::Welcome;
    app.conn_workspace_id = Some(uuid::Uuid::new_v4());
    app.submit_open_local_workspace();
    assert!(matches!(app.screen, Screen::Welcome));
    assert!(app.conn_error.is_some());
    assert_eq!(manager.last_opened_dates().unwrap(), dates);
    // A fresh application reopens the card's stable identity rather than
    // allocating another workspace or rebuilding its planning history.
    let mut restarted = KooladeApp::default();
    let ctx = egui::Context::default();
    click(&mut restarted, &ctx, "Beta workspace");
    assert!(matches!(&restarted.screen, Screen::Connected(project)
        if project.state.planning_store.project_id == second.id));
    assert_eq!(manager.list_local().unwrap().len(), 2);
    drop(restarted);
    drop(app);
    std::fs::remove_dir_all(base).unwrap();
}
