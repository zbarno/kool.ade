use super::{Edge, Node};
use crate::ui::task_detail::TaskRelationView;
use std::collections::{HashMap, HashSet, VecDeque};

#[cfg(test)]
#[path = "descendants/tests.rs"]
mod tests;

pub(super) fn append_descendants(
    ui: &egui::Ui,
    ticket: &str,
    width: f32,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) {
    let children = ui.ctx().data_mut(|data| {
        data.get_temp::<HashMap<String, Vec<TaskRelationView>>>(egui::Id::new(
            "koolade_task_children",
        ))
        .unwrap_or_default()
    });
    let path_uids = ui.ctx().data_mut(|data| {
        data.get_temp::<HashMap<String, String>>(egui::Id::new("koolade_task_uids_by_path"))
            .unwrap_or_default()
    });
    let mut seen = nodes
        .iter()
        .filter_map(|node| {
            node.path
                .as_ref()
                .and_then(|path| path_uids.get(path))
                .cloned()
        })
        .collect::<HashSet<_>>();
    if let Some(root_uid) = path_uids.get(ticket) {
        seen.insert(root_uid.clone());
    }

    let mut queue = VecDeque::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.kind == "Child"
            && let Some(path) = &node.path
        {
            queue.push_back((path.clone(), index, 1usize));
        }
    }
    let mut rows_by_depth = HashMap::<usize, usize>::new();
    while let Some((parent_path, parent_index, depth)) = queue.pop_front() {
        if depth > 12 || nodes.len() >= 512 {
            continue;
        }
        let relations = children.get(&parent_path).cloned().unwrap_or_default();
        for relation in relations {
            if !seen.insert(relation.planning_item_uid.clone()) {
                continue;
            }
            let path = super::relation_path(ui, &relation);
            let index = rows_by_depth.entry(depth).or_default();
            let row = *index;
            *index += 1;
            let node_index = nodes.len();
            nodes.push(Node {
                kind: "Descendant",
                relationship_uid: Some(relation.planning_item_uid.clone()),
                title: relation.title.clone(),
                status: relation.status.clone(),
                path: path.clone(),
                local: egui::vec2(descendant_column_x(width, depth), 24.0 + row as f32 * 82.0),
            });
            edges.push(Edge {
                from: parent_index,
                to: node_index,
                label: "parent of",
                color: egui::Color32::from_rgb(110, 155, 205),
            });
            if let Some(path) = path {
                queue.push_back((path, node_index, depth + 1));
            }
        }
    }
}

pub(super) fn descendant_column_x(width: f32, depth: usize) -> f32 {
    let descendant_column = if width < 560.0 {
        width + 20.0
    } else {
        width - 198.0 + 210.0
    };
    descendant_column + depth.saturating_sub(1) as f32 * 210.0
}
