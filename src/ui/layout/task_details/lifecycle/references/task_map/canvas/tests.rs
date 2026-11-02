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
                paint(ui, &super::super::tests::lifecycle_view(), ticket);
            });
        },
    );
    output.textures_delta.clear();
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
                paint(ui, &super::super::tests::lifecycle_view(), ticket);
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
                    paint(ui, &super::super::tests::lifecycle_view(), ticket);
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
    context.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_task_paths_by_uid"),
            std::collections::HashMap::from([(
                "child-task-uid".to_owned(),
                "planning/tasks/child.md".to_owned(),
            )]),
        );
        data.insert_temp(egui::Id::new("koolade_selected_task"), ticket.to_owned());
    });
    let node_center = egui::pos2(689.0, 165.0);
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
                    paint(ui, &super::super::tests::lifecycle_view(), ticket);
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

#[test]
fn task_map_canvas_stays_hidden_when_a_task_has_no_graph_relationships() {
    let context = egui::Context::default();
    let mut lifecycle = super::super::tests::lifecycle_view();
    lifecycle.parent_task = None;
    lifecycle.dependencies.clear();
    lifecycle.subtasks.clear();
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 480.0),
            )),
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                paint(ui, &lifecycle, "planning/tasks/leaf.md");
            });
        },
    );
    let shows_canvas = output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().contains("Scroll to zoom"))
    });
    output.textures_delta.clear();
    assert!(!shows_canvas);
}

#[test]
fn task_map_canvas_keeps_one_hundred_direct_children_available_to_pan_and_zoom() {
    let context = egui::Context::default();
    let mut lifecycle = super::super::tests::lifecycle_view();
    lifecycle.subtasks = (0..100)
        .map(|index| TaskRelationView {
            planning_item_uid: format!("child-{index}"),
            lifecycle_task_uid: Some(format!("child-task-{index}")),
            title: format!("Child {index}"),
            status: "Running".into(),
            acceptance_criteria: Vec::new(),
        })
        .collect();
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 360.0),
            )),
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                paint(ui, &lifecycle, "planning/tasks/selected.md");
            });
        },
    );
    let child_titles = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text().starts_with("Child ") => {
                Some(text.galley.text().to_owned())
            }
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>();
    let shape_count = output.shapes.len();
    output.textures_delta.clear();
    assert_eq!(child_titles.len(), 100);
    assert!(
        shape_count < 1_000,
        "the bounded canvas emits {shape_count} shapes"
    );
}
