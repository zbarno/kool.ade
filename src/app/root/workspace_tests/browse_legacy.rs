use super::*;

#[test]
fn sw_browse_choice_on_plain_folder_then_open_shows_legacy_invalid_repo_banner() {
    // AC4: choosing a NON-git folder travels the SAME write path as a
    // git tree (canonical PathBuf -> lossy String into conn_path) — the
    // browser neither enables nor disables it. Only the operator's
    // subsequent Open, through the UNCHANGED submit_connect ->
    // welcome::attempt_connect pipeline, reproduces the legacy
    // InvalidRepo banner.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() % 1_000_000_000_000u128)
        .unwrap_or(0);
    let ws = std::env::temp_dir().join(format!("swplain-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(ws.join("plainB")).unwrap();
    let ws_s = ws.to_string_lossy().into_owned();
    let plain_c = std::fs::canonicalize(ws.join("plainB")).unwrap();

    let mut app = KooladeApp {
        conn_path: ws_s.clone(),
        ..Default::default()
    };
    let ctx = egui::Context::default();
    app.dialog = Some(Dialog::Browse(crate::app::dialogs::DlgBrowse::seeded(
        ws_s.clone(),
    )));
    sw_warm_route(&ctx, &mut app);

    // Seeded listing shows plainB; single-click selects, Choose inserts.
    let out = sw_route(&ctx, &mut app, Vec::new());
    let hit = sw_text_pos(&out, "plainB").expect("plainB row painted");
    sw_route_click_at(&ctx, &mut app, hit);
    assert!(app.dialog.is_some(), "selecting keeps the browser open");
    assert_eq!(app.conn_path, ws_s, "a select writes nothing back");
    let out = sw_route(&ctx, &mut app, Vec::new());
    sw_route_click_at(&ctx, &mut app, sw_choose_rect(&out).center());
    assert!(app.dialog.is_none(), "choose consumes the dialog");
    assert_eq!(
        app.conn_path,
        plain_c.to_string_lossy(),
        "the non-git folder is inserted IDENTICALLY to a git tree"
    );
    assert!(
        matches!(app.screen, Screen::Welcome),
        "choose never navigates"
    );

    // The single connect authority runs on the operator's Open: the
    // pre-existing banner surfaces, unchanged.
    app.submit_connect();
    assert!(
        matches!(app.screen, Screen::Welcome),
        "Open refused the non-git folder: still the initial screen"
    );
    let err = app.conn_error.as_deref().unwrap_or("");
    assert!(
        err.contains("no .git directory found"),
        "legacy InvalidRepo banner reproduced (got: {err})"
    );
    assert!(
        err.contains(plain_c.to_str().unwrap_or("")),
        "banner names the chosen path (got: {err})"
    );
    let _ = std::fs::remove_dir_all(&ws);
}
