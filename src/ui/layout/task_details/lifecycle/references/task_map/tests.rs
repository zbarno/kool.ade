use super::{navigate_back, navigate_to, node_status_text, relation_link};

fn pointer_input(position: egui::Pos2, pressed: bool) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(600.0, 400.0),
        )),
        events: vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ],
        ..Default::default()
    }
}

#[test]
fn related_node_statuses_keep_exceptions_visible_without_color() {
    assert_eq!(node_status_text("Running"), "● Running");
    assert_eq!(node_status_text("Blocked"), "⚠ Blocked");
    assert_eq!(
        node_status_text("Waiting for your decision"),
        "✋ Needs user input"
    );
    assert_eq!(node_status_text("Cancelled"), "× Cancelled");
    assert_eq!(node_status_text("Failed"), "! Failed");
    assert_eq!(
        node_status_text("Implementation complete · PR awaiting authorization"),
        "◷ Implementation complete · PR awaiting authorization"
    );
    assert_eq!(
        node_status_text("Implementation complete · PR merged"),
        "✓ Implementation complete · PR merged"
    );
    assert_eq!(node_status_text("Waiting for review"), "◷ Awaiting review");
    assert_eq!(
        node_status_text("Waiting for capacity"),
        "… Waiting for capacity"
    );
    assert_eq!(
        node_status_text("Status not recorded"),
        "? Status not recorded"
    );
}

#[test]
fn clicking_a_recorded_task_node_requests_its_own_modal_path() {
    let context = egui::Context::default();
    let current_path = "parent.md".to_owned();
    let target_path = "child.md".to_owned();
    let task_uid = "child-planning-uid".to_owned();
    let paths_id = egui::Id::new("koolade_task_paths_by_uid");
    let selected_id = egui::Id::new("koolade_selected_task");
    let history_id = egui::Id::new("koolade_task_relationship_history");
    let navigation_id = egui::Id::new("koolade_task_relationship_navigation");
    context.data_mut(|data| {
        data.insert_temp(
            paths_id,
            std::collections::HashMap::from([(task_uid.clone(), target_path.clone())]),
        );
        data.insert_temp(selected_id, current_path.clone());
    });
    let relation = crate::ui::task_detail::TaskRelationView {
        planning_item_uid: task_uid,
        lifecycle_task_uid: Some("child-lifecycle-uid".into()),
        title: "Child task".into(),
        status: "Running".into(),
        acceptance_criteria: Vec::new(),
    };
    let mut initial = context.run_ui(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            relation_link(ui, "Subtask", &relation);
        });
    });
    let center = initial
        .shapes
        .iter()
        .find_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            text.galley
                .text()
                .contains("Subtask Child task")
                .then(|| text.pos + text.galley.mesh_bounds.center().to_vec2())
        })
        .expect("related task node is rendered as a link");
    initial.textures_delta.clear();
    for pressed in [true, false] {
        let mut frame = context.run_ui(pointer_input(center, pressed), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                relation_link(ui, "Subtask", &relation);
            });
        });
        frame.textures_delta.clear();
    }
    assert_eq!(
        context.data_mut(|data| data.get_temp::<String>(navigation_id)),
        Some(target_path)
    );
    assert_eq!(
        context.data_mut(|data| data.get_temp::<Vec<String>>(history_id)),
        Some(vec![current_path])
    );
}

#[test]
fn task_navigation_pushes_current_path_and_back_restores_previous_path() {
    let context = egui::Context::default();
    let selected = egui::Id::new("koolade_selected_task");
    let history = egui::Id::new("koolade_task_relationship_history");
    let navigation = egui::Id::new("koolade_task_relationship_navigation");
    context.data_mut(|data| data.insert_temp(selected, "original.md".to_owned()));
    let mut frame = context.run_ui(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| navigate_to(ui, "child.md".into()));
    });
    assert_eq!(
        context.data_mut(|data| data.get_temp::<Vec<String>>(history)),
        Some(vec!["original.md".to_owned()])
    );
    assert_eq!(
        context.data_mut(|data| data.get_temp::<String>(navigation)),
        Some("child.md".to_owned())
    );
    frame.textures_delta.clear();
    context.data_mut(|data| data.insert_temp(selected, "child.md".to_owned()));
    let _ = context.run_ui(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| navigate_back(ui, "original.md".into()));
    });
    assert!(
        context
            .data_mut(|data| data.get_temp::<Vec<String>>(history))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        context.data_mut(|data| data.get_temp::<String>(navigation)),
        Some("original.md".to_owned())
    );
}
