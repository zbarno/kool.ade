use super::{CanvasState, Node};

pub(super) fn fit_state(nodes: &[Node], canvas_rect: egui::Rect) -> CanvasState {
    let bounds = nodes
        .iter()
        .map(|node| {
            egui::Rect::from_min_size(
                egui::pos2(node.local.x, node.local.y),
                egui::vec2(190.0, 62.0),
            )
        })
        .reduce(egui::Rect::union)
        .unwrap_or(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::Vec2::ZERO,
        ));
    let zoom = ((canvas_rect.width() - 16.0) / bounds.width().max(1.0))
        .min((canvas_rect.height() - 16.0) / bounds.height().max(1.0))
        .clamp(0.02, 2.5);
    CanvasState {
        pan: canvas_rect.size() * 0.5 - bounds.center().to_vec2() * zoom,
        zoom,
    }
}
