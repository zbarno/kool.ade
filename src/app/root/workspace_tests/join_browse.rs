//! Browse-routing tests for the planning-repository card.
//!
//! Production boundary: the welcome screen exposes ONE native folder picker
//! for two destinations (the code-repository field `conn_path` and the
//! planning-repository field `conn_join_remote`). The OS dialog itself is
//! exercised by the user; the router under test here is
//! `KooladeApp::apply_folder_pick`, the seam between the dialog result and
//! the field it must fill. A unit test cannot replace this without leaving
//! the field-dispatch logic unverified — a routing slip would silently send
//! a shared planning checkout into the code-repository field (or vice
//! versa), turning "point the app at a teammate's planning repository" into
//! a corrupt connect attempt.
use super::*;

#[test]
fn sw_join_browse_fill_targets_the_planning_field_only() {
    let mut app = KooladeApp::default();
    let before_conn = app.conn_path.clone();
    let root = std::env::temp_dir().join(format!("join-browse-{}", uuid::Uuid::new_v4()));
    let planning = root.join("planning-repo");
    std::fs::create_dir_all(&planning).unwrap();

    app.apply_folder_pick(BrowseTarget::PlanningRepo, Some(planning.clone()));

    assert_eq!(app.conn_join_remote, planning.display().to_string());
    assert_eq!(app.conn_path, before_conn, "code field must stay untouched");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sw_code_browse_fill_targets_the_code_field_only() {
    let mut app = KooladeApp::default();
    let before_join = app.conn_join_remote.clone();
    let root = std::env::temp_dir().join(format!("join-browse-code-{}", uuid::Uuid::new_v4()));
    let code = root.join("code-repo");
    std::fs::create_dir_all(&code).unwrap();

    app.apply_folder_pick(BrowseTarget::CodeRepo, Some(code.clone()));

    assert_eq!(app.conn_path, code.display().to_string());
    assert_eq!(
        app.conn_join_remote, before_join,
        "planning field must stay untouched"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sw_browse_cancel_leaves_both_fields_untouched() {
    let mut app = KooladeApp {
        conn_path: "/kept/code".into(),
        conn_join_remote: "/kept/planning".into(),
        ..Default::default()
    };

    app.apply_folder_pick(BrowseTarget::PlanningRepo, None);
    app.apply_folder_pick(BrowseTarget::CodeRepo, None);

    assert_eq!(app.conn_path, "/kept/code");
    assert_eq!(app.conn_join_remote, "/kept/planning");
}
