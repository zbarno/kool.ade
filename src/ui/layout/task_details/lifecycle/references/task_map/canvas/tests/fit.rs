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
                local: node_position(640.0, 2, index, count),
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
