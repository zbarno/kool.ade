use super::*;

#[path = "tests/fit.rs"]
mod fit;
#[path = "tests/interaction.rs"]
mod interaction;

#[test]
fn relationship_edge_labels_match_parent_dependency_and_child_direction() {
    let context = egui::Context::default();
    let mut lifecycle = super::super::tests::lifecycle_view();
    lifecycle.parent_task = Some(TaskRelationView {
        planning_item_uid: "parent".into(),
        lifecycle_task_uid: Some("parent".into()),
        title: "Parent".into(),
        status: "Running".into(),
        acceptance_criteria: Vec::new(),
    });
    lifecycle.dependencies = vec![TaskRelationView {
        planning_item_uid: "prerequisite".into(),
        lifecycle_task_uid: Some("prerequisite".into()),
        title: "Prerequisite".into(),
        status: "Completed".into(),
        acceptance_criteria: Vec::new(),
    }];
    context.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_task_dependents"),
            std::collections::HashMap::<String, Vec<TaskRelationView>>::from([(
                "selected.md".into(),
                vec![TaskRelationView {
                    planning_item_uid: "dependent-uid".into(),
                    lifecycle_task_uid: None,
                    title: "Dependent".into(),
                    status: "Blocked".into(),
                    acceptance_criteria: Vec::new(),
                }],
            )]),
        );
    });
    let relation = |title: &str| TaskRelationView {
        planning_item_uid: title.to_owned(),
        lifecycle_task_uid: Some(title.to_owned()),
        title: title.to_owned(),
        status: "Status not recorded".into(),
        acceptance_criteria: Vec::new(),
    };
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 480.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut nodes = vec![Node {
                    kind: "Selected task",
                    relationship_uid: None,
                    title: "Selected".into(),
                    status: "Running".into(),
                    path: Some("selected.md".into()),
                    local: egui::Vec2::ZERO,
                }];
                let mut edges = Vec::new();
                for (kind, direction) in [("Parent", 0), ("Prerequisite", 1), ("Child", 2)] {
                    add_relation(
                        ui,
                        &mut nodes,
                        &mut edges,
                        &relation(kind),
                        kind,
                        Placement {
                            direction,
                            width: 640.0,
                            index: 0,
                            count: 1,
                        },
                    );
                }
                assert_eq!(
                    edges
                        .iter()
                        .map(|edge| (edge.from, edge.to, edge.label))
                        .collect::<Vec<_>>(),
                    [
                        (1, 0, "parent of"),
                        (2, 0, "prerequisite for"),
                        (0, 3, "parent of"),
                    ]
                );
                paint(ui, &lifecycle, "selected.md");
            });
        },
    );
    let edge_labels = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(edge_labels.iter().any(|label| label == "blocks"));
    output.textures_delta.clear();
}

#[test]
fn duplicate_child_relationships_render_one_node_and_edge() {
    let context = egui::Context::default();
    let mut lifecycle = super::super::tests::lifecycle_view();
    lifecycle.subtasks.push(lifecycle.subtasks[0].clone());
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 480.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                paint(ui, &lifecycle, "selected.md");
            });
        },
    );
    let child_labels = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text().contains("Child relationship node") => {
                Some(text.galley.text().to_owned())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    output.textures_delta.clear();
    assert_eq!(child_labels.len(), 1, "duplicate relation renders one node");
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
