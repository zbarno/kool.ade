use super::*;

#[test]
fn fit_state_keeps_one_ten_and_one_hundred_relationship_nodes_inside_the_canvas() {
    let rect = egui::Rect::from_min_size(egui::pos2(40.0, 20.0), egui::vec2(640.0, 320.0));
    for count in [1, 10, 100] {
        let nodes = (0..count)
            .map(|index| Node {
                kind: "Child",
                relationship_uid: Some(format!("child-{index}")),
                title: format!("Child {index}"),
                status: "Running".into(),
                path: Some(format!("child-{index}.md")),
                local: node_position(640.0, 2, index, count, 0.0),
            })
            .collect::<Vec<_>>();
        let state = fit_state(&nodes, rect);

        for node in &nodes {
            let position = screen_position(rect, node.local, state.pan, state.zoom);
            let fitted = egui::Rect::from_min_size(position, egui::vec2(190.0, 62.0) * state.zoom);
            assert!(
                rect.contains_rect(fitted),
                "{count} fitted node {:?} exceeds canvas {:?}",
                node.relationship_uid,
                fitted
            );
        }
        if count == 100 {
            assert!(state.zoom < 0.35, "large graph fits at overview zoom");
        }
    }
}

#[test]
fn narrow_layout_keeps_parent_prerequisites_children_dependents_and_descendants_distinct() {
    use super::super::NarrowRows;

    let width = 320.0;
    let rows = NarrowRows::new(true, 2, 2);
    let mut positions = vec![egui::vec2(width * 0.5 - 95.0, rows.selected)];
    positions.push(node_position(width, 0, 0, 1, rows.parent));
    positions.extend((0..2).map(|index| node_position(width, 1, index, 2, rows.dependencies)));
    positions.extend((0..2).map(|index| node_position(width, 2, index, 2, rows.children)));
    positions.extend((0..2).map(|index| node_position(width, 3, index, 2, rows.dependents)));
    positions.extend((0..2).map(|index| {
        egui::vec2(
            super::super::descendants::descendant_column_x(width, 1),
            24.0 + index as f32 * 82.0,
        )
    }));
    let rects = positions
        .into_iter()
        .map(|position| {
            egui::Rect::from_min_size(egui::pos2(position.x, position.y), egui::vec2(190.0, 62.0))
        })
        .collect::<Vec<_>>();

    for (index, rect) in rects.iter().enumerate() {
        for other in rects.iter().skip(index + 1) {
            assert!(!rect.intersects(*other), "narrow graph nodes overlap");
        }
    }
}
