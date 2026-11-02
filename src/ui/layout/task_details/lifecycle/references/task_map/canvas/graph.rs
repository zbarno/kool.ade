#[derive(Clone, Copy)]
pub(super) struct CanvasState {
    pub(super) pan: egui::Vec2,
    pub(super) zoom: f32,
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
    pub(super) kind: &'static str,
    pub(super) relationship_uid: Option<String>,
    pub(super) title: String,
    pub(super) status: String,
    pub(super) path: Option<String>,
    pub(super) local: egui::Vec2,
}

pub(super) struct Edge {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) label: &'static str,
    pub(super) color: egui::Color32,
}

pub(super) struct Placement {
    pub(super) direction: u8,
    pub(super) width: f32,
    pub(super) index: usize,
    pub(super) count: usize,
    pub(super) narrow_y: f32,
}
