use super::*;

#[test]
fn board_restores_blue_activity_on_cards_and_sums_all_sources() {
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
    for (color, expected) in [(crate::ui::theme::PUNCH, 0), (crate::ui::theme::BLUE, 4)] {
        let lines = output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Path(path)
            if path.points.len() == 60 && path.stroke.color == egui::epaint::ColorMode::Solid(color))).count();
        assert_eq!(
            lines, expected,
            "one header chart and one chart per implementation card"
        );
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

#[test]
fn planning_and_question_cards_show_their_recorded_blue_activity() {
    let mut app = fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    project.task_documents.clear();
    project.state.items = vec![OpenItem::new(
        "CLR-021".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "Product".into(),
        None,
        "Who can join the workspace?".into(),
        "Defines access".into(),
    )];
    project.planning_work = vec![crate::core::planning_work::Work::new(
        "plan-activity".into(),
        "Plan onboarding".into(),
        "Simplify joining a team".into(),
        String::new(),
    )];
    for key in ["CLR-021", "plan-activity"] {
        project
            .activity
            .conversations
            .entry(key.into())
            .or_default()
            .telemetry
            .samples = vec![(100, 4), (101, 2)];
    }
    let ctx = super::mockup_layout::styled_context();
    for _ in 0..3 {
        frame(&mut app, &ctx, vec![]);
    }
    let output = frame(&mut app, &ctx, vec![]);
    let nonflat_charts = output
        .shapes
        .iter()
        .filter(|shape| {
            matches!(&shape.shape,
        egui::Shape::Path(path) if path.points.len() == 60
            && path.stroke.color == egui::epaint::ColorMode::Solid(crate::ui::theme::BLUE)
            && path.points.iter().any(|p| (p.y - path.points[0].y).abs() > 1.0))
        })
        .count();
    assert_eq!(
        nonflat_charts, 2,
        "each planning card shows its frozen history, even without implementation telemetry"
    );
}

#[test]
fn activity_footer_stays_close_to_the_task_content() {
    let mut app = fixture();
    let ctx = super::mockup_layout::styled_context();
    let size = egui::vec2(1280.0, 720.0);
    for _ in 0..3 {
        frame_at(&mut app, &ctx, vec![], size);
    }
    let output = frame_at(&mut app, &ctx, vec![], size);
    let title = text_position(&output, "First task").unwrap();
    let graph = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Path(path)
                if path.points.len() == 60
                    && path.points[0].y > title.y
                    && path.points[0].x < title.x
                    && path.points[59].x > title.x =>
            {
                Some(&path.points)
            }
            _ => None,
        })
        .expect("first card's blue chart must be visible");
    assert!(
        graph.iter().all(|p| p.y < title.y + 160.0),
        "activity footer cannot consume the remaining lane height"
    );
}
