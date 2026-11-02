use crate::ui::task_detail::TaskRelationView;

#[cfg(test)]
#[path = "nested/tests.rs"]
mod tests;

pub(super) fn paint(ui: &mut egui::Ui, relation: &TaskRelationView) {
    let path = ui.ctx().data_mut(|data| {
        data.get_temp::<std::collections::HashMap<String, String>>(egui::Id::new(
            "koolade_task_paths_by_uid",
        ))
        .and_then(|paths| paths.get(&relation.planning_item_uid).cloned())
    });
    if let Some(path) = path {
        paint_children(ui, &path, 0, &mut std::collections::HashSet::new());
    }
}

fn paint_children(
    ui: &mut egui::Ui,
    parent_path: &str,
    depth: usize,
    visited: &mut std::collections::HashSet<String>,
) {
    if depth >= 12 || !visited.insert(parent_path.to_owned()) {
        return;
    }
    let children = ui.ctx().data_mut(|data| {
        data.get_temp::<std::collections::HashMap<String, Vec<TaskRelationView>>>(egui::Id::new(
            "koolade_task_children",
        ))
        .and_then(|children| children.get(parent_path).cloned())
        .unwrap_or_default()
    });
    if !children.is_empty() {
        egui::CollapsingHeader::new(format!("Expand descendants · {} tasks", children.len()))
            .id_salt(("task_nested_children", parent_path))
            .show(ui, |ui| {
                for child in &children {
                    ui.group(|ui| {
                        if let Some(path) = super::canvas::relation_path(ui, child) {
                            if ui.link(format!("↓ {}", child.title)).clicked() {
                                super::navigate_to(ui, path);
                            }
                        } else {
                            ui.label(format!("↓ {}", child.title));
                        }
                        super::node_status(ui, &child.status);
                        if let Some(path) = super::canvas::relation_path(ui, child) {
                            paint_children(ui, &path, depth + 1, visited);
                        }
                    });
                }
            });
    }
    visited.remove(parent_path);
}
