use crate::ui::task_detail::{LifecycleView, TaskRelationView};
mod descendants;
mod fit;
mod layout;
use descendants::append_descendants;
use fit::fit_state;
pub(super) use layout::relation_path;
use layout::{draw_arrow, node_position, screen_position, task_title};

#[derive(Clone, Copy)]
struct CanvasState {
    pan: egui::Vec2,
    zoom: f32,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
        }
    }
}

pub(super) struct Node {
    kind: &'static str,
    relationship_uid: Option<String>,
    title: String,
    status: String,
    path: Option<String>,
    local: egui::Vec2,
}

pub(super) struct Edge {
    from: usize,
    to: usize,
    label: &'static str,
    color: egui::Color32,
}

struct Placement {
    direction: u8,
    width: f32,
    index: usize,
    count: usize,
}

#[cfg(test)]
#[path = "canvas/tests.rs"]
mod tests;

pub(super) fn paint(ui: &mut egui::Ui, lifecycle: &LifecycleView, ticket: &str) {
    let dependents = ui.ctx().data_mut(|data| {
        data.get_temp::<std::collections::HashMap<String, Vec<TaskRelationView>>>(egui::Id::new(
            "koolade_task_dependents",
        ))
        .and_then(|rows| rows.get(ticket).cloned())
        .unwrap_or_default()
    });
    if lifecycle.parent_task.is_none()
        && lifecycle.dependencies.is_empty()
        && lifecycle.subtasks.is_empty()
        && dependents.is_empty()
    {
        return;
    }
    let width = ui.available_width().clamp(320.0, 960.0);
    let mut fit_graph = false;
    ui.horizontal(|ui| {
        fit_graph = ui.button("Fit graph").clicked();
        ui.small("Scroll to zoom · right-drag to pan · double-click to reset");
    });
    let (canvas_rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 320.0), egui::Sense::click_and_drag());
    let id = egui::Id::new(("task_map_canvas_state", ticket));
    let saved_state = ui.ctx().data_mut(|data| data.get_temp::<CanvasState>(id));
    let mut state = saved_state.unwrap_or_default();
    if response.dragged_by(egui::PointerButton::Secondary) {
        state.pan += ui.input(|input| input.pointer.delta());
    }
    if response.hovered() {
        let scroll = ui.input(|input| input.smooth_scroll_delta.y);
        if scroll != 0.0 {
            state.zoom = (state.zoom * (1.0 + scroll * 0.001)).clamp(0.02, 2.5);
        }
    }
    if response.double_clicked() {
        state = CanvasState::default();
    }
    let painter = ui.painter_at(canvas_rect);
    painter.rect_filled(canvas_rect, 6.0, ui.visuals().extreme_bg_color);
    painter.rect_stroke(
        canvas_rect,
        6.0,
        ui.visuals().widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let mut nodes = vec![Node {
        kind: "Selected task",
        relationship_uid: None,
        title: task_title(ui, ticket),
        status: crate::ui::task_detail::task_phase_label(lifecycle.task_phase).to_owned(),
        path: Some(ticket.to_owned()),
        local: egui::vec2(
            width * 0.5 - 95.0,
            if width < 560.0 { 150.0 } else { 126.0 },
        ),
    }];
    let mut edges = Vec::new();
    if let Some(parent) = &lifecycle.parent_task {
        add_relation(
            ui,
            &mut nodes,
            &mut edges,
            parent,
            "Parent",
            Placement {
                direction: 0,
                width,
                index: 0,
                count: 1,
            },
        );
    }
    for (index, dependency) in lifecycle.dependencies.iter().enumerate() {
        add_relation(
            ui,
            &mut nodes,
            &mut edges,
            dependency,
            "Prerequisite",
            Placement {
                direction: 1,
                width,
                index,
                count: lifecycle.dependencies.len(),
            },
        );
    }
    for (index, child) in lifecycle.subtasks.iter().enumerate() {
        add_relation(
            ui,
            &mut nodes,
            &mut edges,
            child,
            "Child",
            Placement {
                direction: 2,
                width,
                index,
                count: lifecycle.subtasks.len(),
            },
        );
    }
    for (index, dependent) in dependents.iter().enumerate() {
        let node_index = nodes.len();
        nodes.push(Node {
            kind: "Dependent",
            relationship_uid: Some(dependent.planning_item_uid.clone()),
            title: dependent.title.clone(),
            status: dependent.status.clone(),
            path: relation_path(ui, dependent),
            local: node_position(width, 3, index, dependents.len()),
        });
        edges.push(Edge {
            from: 0,
            to: node_index,
            label: "blocks",
            color: egui::Color32::from_rgb(205, 144, 72),
        });
    }
    let expand_id = egui::Id::new(("task_map_expand_descendants", ticket));
    let mut expand_descendants = ui
        .ctx()
        .data_mut(|data| data.get_temp::<bool>(expand_id))
        .unwrap_or(false);
    let mut graph_changed = false;
    if !lifecycle.subtasks.is_empty()
        && ui
            .button(if expand_descendants {
                "Collapse descendants"
            } else {
                "Expand descendants in graph"
            })
            .clicked()
    {
        expand_descendants = !expand_descendants;
        graph_changed = true;
    }
    if expand_descendants {
        append_descendants(ui, ticket, width, &mut nodes, &mut edges);
    }
    if fit_graph || graph_changed || saved_state.is_none() {
        state = fit_state(&nodes, canvas_rect);
    }
    for edge in &edges {
        let from = screen_position(canvas_rect, nodes[edge.from].local, state.pan, state.zoom);
        let to = screen_position(canvas_rect, nodes[edge.to].local, state.pan, state.zoom);
        draw_arrow(&painter, from, to, edge.color);
        painter.text(
            from.lerp(to, 0.52) + egui::vec2(0.0, -10.0),
            egui::Align2::CENTER_CENTER,
            edge.label,
            egui::FontId::proportional(10.0),
            edge.color,
        );
    }
    for (index, node) in nodes.iter().enumerate() {
        let pos = screen_position(canvas_rect, node.local, state.pan, state.zoom);
        let rect = egui::Rect::from_min_size(pos, egui::vec2(190.0, 62.0) * state.zoom);
        let fill = if index == 0 {
            ui.visuals().selection.bg_fill
        } else {
            ui.visuals().widgets.inactive.bg_fill
        };
        painter.rect_filled(rect, 5.0, fill);
        painter.rect_stroke(
            rect,
            5.0,
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color),
            egui::StrokeKind::Inside,
        );
        let text_pos = rect.min + egui::vec2(6.0, 4.0) * state.zoom;
        painter.text(
            text_pos,
            egui::Align2::LEFT_TOP,
            node.kind,
            egui::FontId::proportional(10.0 * state.zoom),
            ui.visuals().weak_text_color(),
        );
        painter.text(
            text_pos + egui::vec2(0.0, 14.0) * state.zoom,
            egui::Align2::LEFT_TOP,
            &node.title,
            egui::FontId::proportional(12.0 * state.zoom),
            ui.visuals().text_color(),
        );
        painter.text(
            text_pos + egui::vec2(0.0, 32.0) * state.zoom,
            egui::Align2::LEFT_TOP,
            &node.status,
            egui::FontId::proportional(10.0 * state.zoom),
            ui.visuals().weak_text_color(),
        );
        if index != 0
            && let Some(path) = &node.path
        {
            let click = ui.interact(rect, id.with(("node", index)), egui::Sense::click());
            if click.clicked() {
                super::navigate_to(ui, path.clone());
            }
        }
    }
    ui.ctx().data_mut(|data| data.insert_temp(id, state));
    ui.ctx()
        .data_mut(|data| data.insert_temp(expand_id, expand_descendants));
    ui.small("Click a task node to open its Task Details.");
}

fn add_relation(
    ui: &egui::Ui,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    relation: &TaskRelationView,
    kind: &'static str,
    placement: Placement,
) {
    let relationship_uid = relation
        .lifecycle_task_uid
        .as_deref()
        .unwrap_or(&relation.planning_item_uid);
    if nodes
        .iter()
        .any(|node| node.kind == kind && node.relationship_uid.as_deref() == Some(relationship_uid))
    {
        return;
    }
    let node_index = nodes.len();
    nodes.push(Node {
        kind,
        relationship_uid: Some(relationship_uid.to_owned()),
        title: relation.title.clone(),
        status: relation.status.clone(),
        path: relation_path(ui, relation),
        local: node_position(
            placement.width,
            placement.direction,
            placement.index,
            placement.count,
        ),
    });
    let incoming = placement.direction <= 1;
    edges.push(Edge {
        from: if incoming { node_index } else { 0 },
        to: if incoming { 0 } else { node_index },
        label: if placement.direction == 0 {
            "parent of"
        } else if placement.direction == 1 {
            "prerequisite for"
        } else {
            "parent of"
        },
        color: if placement.direction == 1 {
            egui::Color32::from_rgb(194, 111, 82)
        } else {
            egui::Color32::from_rgb(110, 155, 205)
        },
    });
}
