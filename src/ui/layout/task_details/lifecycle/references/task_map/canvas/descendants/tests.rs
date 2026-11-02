use super::super::{Edge, Node};
use super::append_descendants;
use crate::ui::task_detail::TaskRelationView;
use std::collections::HashMap;

#[test]
fn expanded_canvas_keeps_nested_branches_separate_and_stops_cycles() {
    let context = egui::Context::default();
    let child_a = relation("child-a", "Child A");
    let child_b = relation("child-b", "Child B");
    let grandchild_a = relation("grandchild-a", "Grandchild A");
    let grandchild_b = relation("grandchild-b", "Grandchild B");
    context.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_task_children"),
            HashMap::from([
                ("child-a.md".to_owned(), vec![grandchild_a.clone()]),
                ("child-b.md".to_owned(), vec![grandchild_b.clone()]),
                ("grandchild-a.md".to_owned(), vec![child_a.clone()]),
                ("grandchild-b.md".to_owned(), vec![child_b.clone()]),
            ]),
        );
        data.insert_temp(
            egui::Id::new("koolade_task_paths_by_uid"),
            HashMap::from([
                ("child-a".to_owned(), "child-a.md".to_owned()),
                ("child-b".to_owned(), "child-b.md".to_owned()),
                ("grandchild-a".to_owned(), "grandchild-a.md".to_owned()),
                ("grandchild-b".to_owned(), "grandchild-b.md".to_owned()),
            ]),
        );
        data.insert_temp(
            egui::Id::new("koolade_task_uids_by_path"),
            HashMap::from([
                ("selected.md".to_owned(), "selected".to_owned()),
                ("child-a.md".to_owned(), "child-a".to_owned()),
                ("child-b.md".to_owned(), "child-b".to_owned()),
                ("grandchild-a.md".to_owned(), "grandchild-a".to_owned()),
                ("grandchild-b.md".to_owned(), "grandchild-b".to_owned()),
            ]),
        );
    });
    let mut nodes = vec![
        Node {
            kind: "Selected task",
            relationship_uid: None,
            title: "Selected".into(),
            status: "Running".into(),
            path: Some("selected.md".into()),
            local: egui::vec2(225.0, 126.0),
        },
        Node {
            kind: "Child",
            relationship_uid: Some("child-a-task".into()),
            title: child_a.title,
            status: child_a.status,
            path: Some("child-a.md".into()),
            local: egui::vec2(442.0, 89.0),
        },
        Node {
            kind: "Child",
            relationship_uid: Some("child-b-task".into()),
            title: child_b.title,
            status: child_b.status,
            path: Some("child-b.md".into()),
            local: egui::vec2(442.0, 163.0),
        },
    ];
    let mut edges = Vec::<Edge>::new();
    let mut output = context.run_ui(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            append_descendants(ui, "selected.md", 640.0, &mut nodes, &mut edges);
        });
    });
    output.textures_delta.clear();

    assert_eq!(nodes.len(), 5, "cycles back to either child are suppressed");
    assert_eq!(nodes[3].title, "Grandchild A");
    assert_eq!(nodes[4].title, "Grandchild B");
    assert_eq!(edges.len(), 2);
    let rects = nodes
        .iter()
        .map(|node| {
            egui::Rect::from_min_size(
                egui::pos2(node.local.x, node.local.y),
                egui::vec2(190.0, 62.0),
            )
        })
        .collect::<Vec<_>>();
    for (index, rect) in rects.iter().enumerate() {
        for other in rects.iter().skip(index + 1) {
            assert!(!rect.intersects(*other), "graph node rectangles overlap");
        }
    }
}

fn relation(uid: &str, title: &str) -> TaskRelationView {
    TaskRelationView {
        planning_item_uid: uid.to_owned(),
        lifecycle_task_uid: None,
        title: title.to_owned(),
        status: "Running".to_owned(),
        acceptance_criteria: Vec::new(),
    }
}
