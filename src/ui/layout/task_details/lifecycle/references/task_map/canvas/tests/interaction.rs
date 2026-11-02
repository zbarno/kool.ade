use super::*;

#[test]
fn task_map_canvas_zoom_responds_to_scroll_over_the_graph() {
    let context = egui::Context::default();
    let ticket = "planning/tasks/selected.md";
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 480.0),
            )),
            events: vec![egui::Event::PointerMoved(egui::pos2(400.0, 160.0))],
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                paint(ui, &super::super::super::tests::lifecycle_view(), ticket);
            });
        },
    );
    output.textures_delta.clear();
    let initial_state = context.data_mut(|data| {
        data.get_temp::<CanvasState>(egui::Id::new(("task_map_canvas_state", ticket)))
    });
    assert!(
        initial_state.expect("initial graph view is persisted").zoom > 1.0,
        "the first canvas frame fits the selected task and its immediate child"
    );
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 480.0),
            )),
            events: vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 120.0),
                phase: egui::TouchPhase::Move,
                modifiers: Default::default(),
            }],
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                paint(ui, &super::super::super::tests::lifecycle_view(), ticket);
            });
        },
    );
    output.textures_delta.clear();
    let state = context.data_mut(|data| {
        data.get_temp::<CanvasState>(egui::Id::new(("task_map_canvas_state", ticket)))
    });
    assert!(state.expect("canvas state is persisted").zoom > 1.0);
    for events in [
        vec![egui::Event::PointerButton {
            pos: egui::pos2(400.0, 160.0),
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: Default::default(),
        }],
        vec![egui::Event::PointerMoved(egui::pos2(430.0, 180.0))],
        vec![egui::Event::PointerButton {
            pos: egui::pos2(430.0, 180.0),
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: Default::default(),
        }],
    ] {
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 480.0),
                )),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    paint(ui, &super::super::super::tests::lifecycle_view(), ticket);
                });
            },
        );
        output.textures_delta.clear();
    }
    let state = context.data_mut(|data| {
        data.get_temp::<CanvasState>(egui::Id::new(("task_map_canvas_state", ticket)))
    });
    assert!(state.expect("canvas state is persisted").pan.length() > 0.0);
}

#[test]
fn clicking_a_canvas_task_node_requests_its_own_task_path() {
    let context = egui::Context::default();
    let ticket = "planning/tasks/selected.md";
    let mut lifecycle = super::super::super::tests::lifecycle_view();
    lifecycle.subtasks[0].planning_item_uid = "child-planning-uid".into();
    lifecycle.subtasks[0].lifecycle_task_uid = Some("child-task-uid".into());
    context.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_task_paths_by_uid"),
            std::collections::HashMap::from([(
                "child-planning-uid".to_owned(),
                "planning/tasks/child.md".to_owned(),
            )]),
        );
        data.insert_temp(egui::Id::new("koolade_selected_task"), ticket.to_owned());
    });
    let mut initial = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 480.0),
            )),
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                paint(ui, &lifecycle, ticket);
            });
        },
    );
    let node_center = initial
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Child relationship node" => {
                Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
            }
            _ => None,
        })
        .expect("fitted canvas paints the child title");
    initial.textures_delta.clear();
    for event in [
        egui::Event::PointerMoved(node_center),
        egui::Event::PointerButton {
            pos: node_center,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerButton {
            pos: node_center,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        },
    ] {
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 480.0),
                )),
                events: vec![event],
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    paint(ui, &lifecycle, ticket);
                });
            },
        );
        output.textures_delta.clear();
    }
    let path = context.data_mut(|data| {
        data.get_temp::<String>(egui::Id::new("koolade_task_relationship_navigation"))
    });
    assert_eq!(path.as_deref(), Some("planning/tasks/child.md"));
}
