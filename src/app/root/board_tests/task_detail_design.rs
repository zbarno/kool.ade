use super::*;

#[test]
fn task_popup_prioritizes_human_title_and_keeps_live_action_visible() {
    for size in [egui::vec2(1280.0, 720.0), egui::vec2(360.0, 720.0)] {
        let mut app = fixture();
        let key = ".koolade-packet/planning/tasks/fixture/F7-TASK-compute-day-and-week-totals";
        let ticket = format!("{key}.md");
        let full_title = "F7-TASK-compute-day-and-week-totals — Compute day and ISO-week per-workspace totals from ledger intervals";
        let Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        project.task_documents[0].path = ticket.clone();
        project.task_documents[0].title = full_title.into();
        project.active_implementations.insert(
            ticket.clone(),
            crate::core::implementation::Controller::idle_fixture(),
        );
        project.activity.tasks.insert(
            ticket.clone(),
            crate::harness::LiveProgress {
                thoughts: "Inspecting the latest check results".into(),
                activity: Some(format!("Implementing {ticket} (attempt 1)…")),
                ..Default::default()
            },
        );
        let ctx = super::mockup_layout::styled_context();
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_selected_task"), ticket.clone())
        });
        for _ in 0..3 {
            frame_at(&mut app, &ctx, vec![], size);
        }
        let output = frame_at(&mut app, &ctx, vec![], size);
        assert!(text_position(&output, "Task details").is_some());
        assert!(
            text_position(&output, full_title).is_none(),
            "slug must not repeat in the heading"
        );
        let stop = text_position(&output, "Stop task and pause queue").expect("live task action");
        assert!(
            stop.x < size.x && stop.y < size.y - 25.0,
            "action must remain visible: {stop:?}"
        );
        assert!(text_position(&output, "Worker detail").is_some());
        assert!(
            !text_contains(&output, &format!("Implementing {ticket}")),
            "raw path stays in the disclosure"
        );
    }
}

#[test]
fn task_details_use_a_wide_workspace_and_stack_cleanly_on_laptop_widths() {
    for size in [egui::vec2(1280.0, 820.0), egui::vec2(900.0, 720.0)] {
        let mut app = fixture();
        let ctx = super::mockup_layout::styled_context();
        for _ in 0..3 {
            frame_at(&mut app, &ctx, vec![], size);
        }
        let output = click_text_at(&mut app, &ctx, "First task", size);
        let details = text_position(&output, "Task details & state").expect("details");
        fn modal_frame(shape: &egui::Shape) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Rect(rect)
                    if rect.corner_radius.nw == 12 && rect.stroke.width == 1.5 =>
                {
                    Some(rect.rect)
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(modal_frame),
                _ => None,
            }
        }
        let modal = output
            .shapes
            .iter()
            .find_map(|shape| modal_frame(&shape.shape))
            .expect("shared task workspace frame");
        assert!(
            egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(modal),
            "modal outside {size:?}: {modal:?}"
        );
        if size.x >= 1200.0 {
            let conversation = text_position(&output, "Task conversation").expect("conversation");
            assert!(
                modal.width() >= size.x * 0.9,
                "workspace too narrow: {modal:?}"
            );
            assert!(
                conversation.x < details.x,
                "desktop panes should be side by side"
            );
        } else {
            assert!(details.x > modal.left() && details.x < modal.right());
            let mut conversation = None;
            for _ in 0..12 {
                let output = frame_at(
                    &mut app,
                    &ctx,
                    vec![
                        egui::Event::PointerMoved(modal.center()),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -1200.0),
                            phase: egui::TouchPhase::Move,
                            modifiers: Default::default(),
                        },
                    ],
                    size,
                );
                if let Some(position) = text_position(&output, "Task conversation") {
                    conversation = Some(position);
                    break;
                }
            }
            let conversation = conversation.expect("stacked conversation is reachable by scroll");
            assert!(
                (conversation.x - details.x).abs() < 80.0,
                "narrow layout should stack the panes in one column"
            );
        }
    }
}

#[test]
fn task_and_planning_cards_share_modal_header_status_and_close_behavior() {
    let size = egui::vec2(1280.0, 820.0);
    let mut task_app = fixture();
    let task_ctx = super::mockup_layout::styled_context();
    for _ in 0..3 {
        frame_at(&mut task_app, &task_ctx, vec![], size);
    }
    let task = click_text_at(&mut task_app, &task_ctx, "Review task", size);
    assert!(text_position(&task, "Task details").is_some());
    assert!(text_position(&task, "CURRENT STATE").is_some());
    assert!(text_position(&task, "Open PR").is_some());
    let task_modal = modal_frame(&task).expect("task uses the shared modal frame");
    assert_modal_escapes(&mut task_app, &task_ctx, size);
    assert!(
        text_position(
            &frame_at(&mut task_app, &task_ctx, vec![], size),
            "Task details"
        )
        .is_none(),
        "Escape closes the selected task modal"
    );

    let mut planning_app = fixture();
    let mut item = OpenItem::new(
        "CLR-010".into(),
        crate::domain::item::Priority::High,
        crate::domain::item::ItemKind::Question,
        "General".into(),
        Some("All".into()),
        "Which users need access?".into(),
        "Determines the access model".into(),
    );
    item.authority = crate::domain::Authority::Human;
    if let Screen::Connected(project) = &mut planning_app.screen {
        project.task_documents.clear();
        project.state.items = vec![item.clone()];
    }
    let planning_ctx = super::mockup_layout::styled_context();
    for _ in 0..3 {
        frame_at(&mut planning_app, &planning_ctx, vec![], size);
    }
    let planning = click_text_at(&mut planning_app, &planning_ctx, &item.question, size);
    assert!(text_position(&planning, "Planning · CLR-010").is_some());
    assert!(text_contains(&planning, "Question · General"));
    assert!(text_position(&planning, "Send answer").is_some());
    let planning_modal = modal_frame(&planning).expect("planning uses the shared modal frame");
    assert_eq!(task_modal.size(), planning_modal.size());
    assert_eq!(task_modal.left(), planning_modal.left());
    assert_eq!(task_modal.top(), planning_modal.top());
    assert_modal_escapes(&mut planning_app, &planning_ctx, size);
    assert!(
        text_position(
            &frame_at(&mut planning_app, &planning_ctx, vec![], size),
            "Planning · CLR-010"
        )
        .is_none(),
        "Escape closes the selected planning modal"
    );
}

fn modal_frame(output: &egui::FullOutput) -> Option<egui::Rect> {
    fn find(shape: &egui::Shape) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Rect(rect) if rect.corner_radius.nw == 12 && rect.stroke.width == 1.5 => {
                Some(rect.rect)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(find),
            _ => None,
        }
    }
    output.shapes.iter().find_map(|shape| find(&shape.shape))
}

fn assert_modal_escapes(app: &mut KooladeApp, ctx: &egui::Context, size: egui::Vec2) {
    frame_at(
        app,
        ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        size,
    );
}
