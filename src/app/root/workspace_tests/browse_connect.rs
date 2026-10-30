use super::*;

#[test]
fn sw_browse_choice_writes_conn_path_only_then_existing_submit_connects() {
    // Serialize ambient-environment mutations (git hierarchy + per-user
    // state root) behind the house lock while this test runs a REAL
    // connect inside a sandbox.
    let _environment = crate::test_support::lock_process_environment();
    let _guard = crate::core::gitops::test_support::shield("sw-browse-glue");
    let _restore_home = RestoreKooladeHome::capture();

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() % 1_000_000_000_000u128)
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!("swglue-{}-{nanos}", std::process::id()));
    let pkg_home = root.join("pkghome");
    std::fs::create_dir_all(&pkg_home).unwrap();
    // SAFETY: the shield is held; no other test observes the per-user
    // state root while this one runs.
    unsafe {
        std::env::set_var("KOOLADE_HOME", &pkg_home);
    }

    let ws = root.join("ws");
    std::fs::create_dir_all(ws.join("plain")).unwrap();
    let repo = ws.join("site-app");
    std::fs::create_dir_all(&repo).unwrap();
    let repo_s = repo.to_string_lossy().into_owned();
    let st = std::process::Command::new("git")
        .args(["-C", &repo_s, "init", "-q"])
        .status()
        .unwrap();
    assert!(st.success(), "git init fixture failed");
    for (k, v) in [
        ("user.name", "Sw Glue Test"),
        ("user.email", "sw-glue@example.invalid"),
        ("commit.gpgsign", "false"),
    ] {
        let c = std::process::Command::new("git")
            .args(["-C", &repo_s, "config", k, v])
            .status()
            .unwrap();
        assert!(c.success(), "git config {k} failed");
    }
    // A mature worktree, the way existing operators' repos look: its
    // product specification was ALREADY migrated to modules and
    // checkpointed, so the (unchanged) connect pipeline's
    // bootstrap/migrate checkpoint deals only in files that exist.
    std::fs::create_dir_all(repo.join("planning")).unwrap();
    let template = crate::artifacts::spec_doc::bootstrap_template("Site App");
    std::fs::write(repo.join("planning/specification.md"), &template).unwrap();
    crate::artifacts::product_docs::migrate(&repo, &template)
        .expect("fixture pre-migration failed");
    for verb in [
        &["add", "-A"][..],
        &["commit", "-q", "-m", "baseline: mature workspace"][..],
    ] {
        let c = std::process::Command::new("git")
            .args(["-C", &repo_s])
            .args(verb)
            .status()
            .unwrap();
        assert!(c.success(), "fixture git step {:?} failed", verb);
    }
    let ws_s = ws.to_string_lossy().into_owned();
    let repo_c = std::fs::canonicalize(&repo).unwrap();

    let mut app = KooladeApp {
        conn_path: ws_s.clone(),
        conn_error: Some(String::from("prior-error-note")),
        ..Default::default()
    };

    let ctx = egui::Context::default();

    // Park the dialog the way the welcome arm does: a fresh browser
    // seeded from the CURRENT field contents. Burn the fresh context\u{2019}s
    // placeholder-only first pass, then idle: the router re-parks and
    // nothing else moves.
    app.dialog = Some(Dialog::Browse(crate::app::dialogs::DlgBrowse::seeded(
        app.conn_path.clone(),
    )));
    sw_warm_route(&ctx, &mut app);
    let out = sw_route(&ctx, &mut app, Vec::new());
    assert!(
        app.dialog.is_some(),
        "an idle dialog frame parks the modal back"
    );
    assert_eq!(app.conn_path, ws_s, "idle frame never touches conn_path");
    assert_eq!(app.conn_error.as_deref(), Some("prior-error-note"));

    // Drive the AC: the seeded workspace lists its children (site-app, a
    // git working tree); single-click it to select, then press
    // \u{201c}Choose folder\u{201d}. Selection writes nothing until Choose.
    let site =
        sw_text_pos(&out, "site-app").expect("the browser lists the seeded workspace contents");
    let choose_at = sw_choose_rect(&out).center();
    sw_route_click_at(&ctx, &mut app, site);
    assert!(
        app.dialog.is_some(),
        "selecting alone keeps the browser open"
    );
    assert_eq!(
        app.conn_path, ws_s,
        "a single-click select is not written back"
    );
    sw_route_click_at(&ctx, &mut app, choose_at);
    assert!(
        app.dialog.is_none(),
        "a chosen dialog is consumed, not parked back"
    );
    assert_eq!(
        app.conn_path,
        repo_c.to_string_lossy(),
        "the chosen canonical path lands in the field verbatim"
    );
    assert_eq!(
        app.conn_error.as_deref(),
        Some("prior-error-note"),
        "choose never touches conn_error"
    );
    assert!(
        matches!(app.screen, Screen::Welcome),
        "choose never navigates or connects"
    );

    // From here the flow is the PRE-EXISTING submit path, unchanged:
    // Open/Enter on this field connects exactly like a hand-typed path —
    // which, for this planning-less fixture, means the planning-setup
    // gate refusal, carried as a banner without touching the field.
    app.submit_connect();
    assert!(
        matches!(app.screen, Screen::Welcome),
        "a planning-less checkout stops at the setup gate (err={:?})",
        app.conn_error
    );
    assert_eq!(
        app.conn_path,
        repo_c.to_string_lossy(),
        "a refused connect preserves the chosen path for setup"
    );
    let banner = app.conn_error.as_deref().unwrap_or("");
    assert!(
        banner.contains("planning repository"),
        "the refusal guides the operator to planning setup (got: {banner})"
    );

    // Reject paths (fresh app): Cancel, the close \u{2715}, and Escape all
    // consume the dialog leaving the field byte-identical.
    let mut app2 = KooladeApp {
        conn_path: ws_s.clone(),
        conn_error: Some(String::from("prior-error-note")),
        ..Default::default()
    };

    app2.dialog = Some(Dialog::Browse(crate::app::dialogs::DlgBrowse::seeded(
        ws_s.clone(),
    )));
    sw_route_click_by_label(&ctx, &mut app2, "Cancel");
    assert!(app2.dialog.is_none());
    assert_eq!(
        app2.conn_path, ws_s,
        "Cancel leaves the typed path untouched"
    );
    assert_eq!(app2.conn_error.as_deref(), Some("prior-error-note"));

    // Closing the modal \u{2715} (unlabeled X-shape): click its derived centre.
    app2.dialog = Some(Dialog::Browse(crate::app::dialogs::DlgBrowse::seeded(
        ws_s.clone(),
    )));
    let out = sw_route(&ctx, &mut app2, Vec::new());
    fn modal_rect(shape: &egui::Shape) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Rect(rect)
                if (rect.corner_radius.nw as f32 - 12.0).abs() < 1.01
                    && rect.stroke.width >= 1.0 =>
            {
                Some(rect.rect)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(modal_rect),
            _ => None,
        }
    }
    let panel = out
        .shapes
        .iter()
        .find_map(|shape| modal_rect(&shape.shape))
        .unwrap_or_else(|| panic!("modal panel frame missing from shapes"));
    sw_route_click_at(&ctx, &mut app2, sw_close_pos(panel));
    assert!(
        app2.dialog.is_none(),
        "the close \u{2715} dismisses the modal"
    );
    assert_eq!(
        app2.conn_path, ws_s,
        "close \u{2715} leaves the typed path untouched"
    );

    app2.dialog = Some(Dialog::Browse(crate::app::dialogs::DlgBrowse::seeded(
        ws_s.clone(),
    )));
    let _ = sw_route(
        &ctx,
        &mut app2,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            modifiers: Default::default(),
            pressed: true,
            repeat: false,
        }],
    );
    assert!(app2.dialog.is_none());
    assert_eq!(
        app2.conn_path, ws_s,
        "Escape leaves the typed path untouched"
    );

    let _ = std::fs::remove_dir_all(&root);
}
