use super::*;

#[test]
fn completed_task_shows_cleanup_failure_without_reopening_implementation() {
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        let record = p
            .implementation_states
            .get_mut(".koolade-packet/planning/tasks/fixture/003-task.md")
            .unwrap();
        record.cleanup.error = Some("Task repository contains local changes".into());
    }
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = click_text(&mut app, &ctx, "Merged task");
    assert!(text_position(&output, "Task details & state").is_some());
    assert!(text_contains(&output, "cleanup needs attention"));
    assert!(text_contains(
        &output,
        "Task repository contains local changes"
    ));
}

#[test]
fn failed_task_without_saved_state_shows_cause_on_board() {
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        p.queue.blocked.insert(
            p.task_documents[0].path.clone(),
            crate::core::implementation::Failure::other("No space left on device"),
        );
    }
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = click_text(&mut app, &ctx, "First task");
    assert!(text_position(&output, "Needs attention · 1").is_some());
    assert!(text_position(&output, "No space left on device").is_some());
    assert!(text_position(&output, "Full blocker report").is_some());
    let report = click_text(&mut app, &ctx, "Full blocker report");
    assert!(text_contains(&report, "No space left on device"));
}
