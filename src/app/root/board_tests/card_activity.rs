use super::*;

#[test]
fn board_keeps_activity_details_out_of_cards_and_sums_all_sources() {
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        for (key, count) in [
            (".koolade-packet/planning/tasks/fixture/001-task.md", 2),
            ("another-task", 3),
        ] {
            p.activity
                .tasks
                .entry(key.into())
                .or_default()
                .telemetry
                .samples = vec![(100, count)];
        }
        p.activity
            .conversations
            .entry("__main".into())
            .or_default()
            .telemetry
            .samples = vec![(100, 5), (101, 1)];
        p.activity
            .conversations
            .entry(".koolade-packet/planning/tasks/fixture/001-task.md".into())
            .or_default()
            .telemetry
            .samples = vec![(100, 7)];
        p.active_implementations.insert(
            ".koolade-packet/planning/tasks/fixture/001-task.md".into(),
            crate::core::implementation::Controller::idle_fixture(),
        );
    }
    assert_eq!(app.activity_samples(None), vec![(100, 17), (101, 1)]);
    assert_eq!(
        app.activity_samples(Some(".koolade-packet/planning/tasks/fixture/001-task.md")),
        vec![(100, 9)]
    );
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Live activity").is_some());
    for (color, expected) in [(crate::ui::theme::PUNCH, 1), (crate::ui::theme::BLUE, 0)] {
        let lines = output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Path(path)
            if path.points.len() == 60 && path.stroke.color == egui::epaint::ColorMode::Solid(color))).count();
        assert_eq!(lines, expected, "card activity stays in Task Details");
    }
    assert!(text_position(&output, "No activity yet").is_none());
    // Starting another run must not erase the project's observed history.
    if let Screen::Connected(p) = &mut app.screen {
        p.activity.ensure_overall();
        p.activity.tasks.clear();
        p.activity.conversations.clear();
    }
    assert_eq!(app.activity_samples(None), vec![(100, 17), (101, 1)]);
}

#[test]
fn compact_task_cards_only_show_inputs_for_pending_answers() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Add context").is_none());
    assert!(text_position(&output, "Your answer…").is_none());
    assert!(text_position(&output, "Send answer").is_none());
    assert!(text_position(&output, "First task").is_some());
    click_text(&mut app, &ctx, "First task");
    let details = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&details, "Your response…").is_some());
    assert!(text_position(&details, "Send response").is_some());
}

#[test]
fn done_task_can_be_archived_off_the_board() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Merged task").is_some());
    assert!(text_position(&output, "Archive").is_none());
    click_text(&mut app, &ctx, "Merged task");
    click_text(&mut app, &ctx, "Archive");
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Merged task").is_none());
    let Screen::Connected(project) = &app.screen else {
        panic!("disconnected")
    };
    let ticket = ".koolade-packet/planning/tasks/fixture/003-task.md";
    assert!(project.archived_tasks.contains(ticket));
    assert!(crate::persistence::archived_tasks::load(&project.chat_slug).contains(ticket));
}
