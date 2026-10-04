use super::*;

#[test]
fn narrow_workspace_keeps_board_primary_without_main_chat() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    let size = egui::vec2(360.0, 480.0);
    frame_at(&mut app, &ctx, vec![], size);
    let output = frame_at(&mut app, &ctx, vec![], size);
    let board = text_position(&output, "Board  3").unwrap();
    assert!(text_position(&output, "Main Chat").is_none());
    assert!(text_position(&output, "+ New Task").is_some());
    assert!(board.x < size.x && board.y < size.y);
    assert_eq!(output.viewport_output.len(), 1);
}

#[test]
fn legacy_main_chat_drafts_remain_hidden_at_narrow_and_wide_sizes() {
    for size in [egui::vec2(360.0, 480.0), egui::vec2(1480.0, 900.0)] {
        let mut app = fixture();
        let ctx = egui::Context::default();
        let draft = "A long wrapped draft with several words on every line.\n".repeat(150);
        *app.chat_draft() = draft.clone();
        for _ in 0..3 {
            frame_at(&mut app, &ctx, vec![], size);
        }
        let output = frame_at(&mut app, &ctx, vec![], size);
        assert!(text_position(&output, "Main Chat").is_none());
        assert!(text_position(&output, &draft).is_none());
        assert!(text_position(&output, "Ctrl + Enter to send").is_none());
        assert_eq!(app.chat_draft(), &draft);
    }
}

#[test]
fn pending_repository_refresh_does_not_block_ui_interactions() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    let (release, wait) = std::sync::mpsc::channel::<()>();
    app.display_refresh = Some(std::thread::spawn(move || {
        let _ = wait.recv();
        panic!("test worker has no snapshot")
    }));
    app.last_git_refresh = Instant::now() - Duration::from_secs(10);
    app.tick(0.016, &ctx);
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Workspace");
    let output = click_text(&mut app, &ctx, "Settings…");
    assert!(text_position(&output, "Workspace settings").is_some());
    assert!(!app.display_refresh.as_ref().unwrap().is_finished());
    release.send(()).unwrap();
    let _ = app.display_refresh.take().unwrap().join();
}
