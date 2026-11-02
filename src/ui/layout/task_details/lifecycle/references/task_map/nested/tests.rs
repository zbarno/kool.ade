use super::*;

#[test]
fn expanded_cyclic_child_relationship_stops_at_the_visited_task() {
    let context = egui::Context::default();
    context.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_task_paths_by_uid"),
            std::collections::HashMap::<String, String>::from([
                ("root-planning-uid".into(), "root.md".into()),
                ("child-planning-uid".into(), "child.md".into()),
            ]),
        );
        data.insert_temp(
            egui::Id::new("koolade_task_children"),
            std::collections::HashMap::<String, Vec<TaskRelationView>>::from([
                (
                    "root.md".into(),
                    vec![TaskRelationView {
                        planning_item_uid: "child-planning-uid".into(),
                        lifecycle_task_uid: Some("child-lifecycle-uid".into()),
                        title: "Child task".into(),
                        status: "Running".into(),
                        acceptance_criteria: Vec::new(),
                    }],
                ),
                (
                    "child.md".into(),
                    vec![TaskRelationView {
                        planning_item_uid: "root-planning-uid".into(),
                        lifecycle_task_uid: Some("root-lifecycle-uid".into()),
                        title: "Cycle back to root".into(),
                        status: "Running".into(),
                        acceptance_criteria: Vec::new(),
                    }],
                ),
            ]),
        );
    });
    let relation = TaskRelationView {
        planning_item_uid: "root-planning-uid".into(),
        lifecycle_task_uid: Some("root-lifecycle-uid".into()),
        title: "Root task".into(),
        status: "Running".into(),
        acceptance_criteria: Vec::new(),
    };
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                paint(ui, &relation);
            });
        },
    );
    let texts = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    output.textures_delta.clear();

    assert!(texts.iter().any(|text| text.contains("Child task")));
    assert!(texts.iter().any(|text| text.contains("Cycle back to root")));
    assert!(
        texts.len() < 20,
        "cyclic expansion stays bounded: {texts:?}"
    );
}
