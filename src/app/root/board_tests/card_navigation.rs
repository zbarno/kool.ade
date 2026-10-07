use super::*;

#[test]
fn board_cards_select_story_details_and_open_the_correct_pr() {
    let mut app = fixture();
    let ctx = egui::Context::default();
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("koolade_document_tab"), true));
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    for label in [
        "To do · 1",
        "In progress · 0",
        "In review · 1",
        "Needs attention · 0",
        "Done · 1",
    ] {
        assert!(text_position(&output, label).is_some(), "missing {label}");
    }
    assert!(
        text_position(&output, "Unique story detail 1").is_none(),
        "Details must not appear beneath the board"
    );
    for shape in &output.shapes {
        if let egui::Shape::Rect(rect) = &shape.shape
            && rect.corner_radius.nw == 8
            && rect.fill == crate::ui::theme::BG
        {
            assert!(
                rect.rect.right() <= 1773.0,
                "Board column overflows the main panel: {:?}",
                rect.rect
            );
        }
    }
    let click = text_position(&output, "Review task").unwrap();
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(click),
            egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerButton {
            pos: click,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let output = frame(&mut app, &ctx, vec![]);
    assert_eq!(
        ctx.data_mut(|d| d.get_temp::<String>(egui::Id::new("koolade_selected_task")))
            .as_deref(),
        Some(".koolade-packet/planning/tasks/fixture/002-task.md")
    );
    assert!(
        text_position(&output, "Unique story detail 1").is_none(),
        "texts: {:?}",
        output
            .shapes
            .iter()
            .filter_map(|shape| if let egui::Shape::Text(t) = &shape.shape {
                Some((t.galley.text(), t.pos))
            } else {
                None
            })
            .collect::<Vec<_>>()
    );
    let link = text_position(&output, "Open PR").unwrap();
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(link),
            egui::Event::PointerButton {
                pos: link,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    let output = frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerButton {
            pos: link,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert!(output.platform_output.commands.iter().any(|cmd| matches!(cmd, egui::OutputCommand::OpenUrl(url) if url.url == "https://github.com/fixture/repo/pull/1")));
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Unique story detail 1").is_none());
    assert!(
        ctx.data_mut(|d| d.get_temp::<String>(egui::Id::new("koolade_selected_task")))
            .is_none()
    );
    app.implement_task(".koolade-packet/planning/tasks/fixture/002-task.md".into());
    assert!(
        !app.is_busy(),
        "published tasks must not start another agent"
    );
}
#[test]
fn live_card_opens_full_activity_and_returns_to_item_details() {
    let mut app = fixture();
    let ticket = ".koolade-packet/planning/tasks/fixture/001-task.md".to_owned();
    if let Screen::Connected(p) = &mut app.screen {
        p.active_implementations.insert(
            ticket.clone(),
            crate::core::implementation::Controller::idle_fixture(),
        );
        p.activity.tasks.insert(
            ticket.clone(),
            crate::harness::LiveProgress {
                thoughts: "Checking the permissions test results".into(),
                activity: Some("Running tests".into()),
                ..Default::default()
            },
        );
    }
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "LIVE ACTIVITY").is_some());
    assert!(text_position(&output, "Checking the permissions test results").is_none());
    let click = |app: &mut KooladeApp, pos: egui::Pos2| {
        for pressed in [true, false] {
            frame(
                app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    };
    click(&mut app, text_position(&output, "First task").unwrap());
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Activity").is_some());
    assert!(text_position(&output, "Checking the permissions test results").is_some());
    click(
        &mut app,
        text_position(&output, "View all activity").unwrap(),
    );
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "TASK-001 / All activity").is_some());
    assert!(
        app.live_progress().is_none(),
        "Task activity must not leak to main chat"
    );
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    let output = frame(&mut app, &ctx, vec![]);
    assert!(
        ctx.data_mut(|d| d.get_temp::<String>(egui::Id::new("koolade_task_activity")))
            .is_none()
    );
    assert!(text_position(&output, "Task details").is_none());
}
