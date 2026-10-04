//! Kool.ad/e visual identity and reusable surface styling.

use egui::{Color32, CornerRadius, Stroke};

pub const REDUCE_MOTION_ID: &str = "kool_ade_reduce_motion";

pub fn reduced_motion(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| {
        data.get_temp::<bool>(egui::Id::new(REDUCE_MOTION_ID))
            .unwrap_or(false)
    })
}

/// Keep active work legible without rotating indicators when reduced motion
/// is enabled. Data graphs continue to update from their measured samples.
pub fn operation_indicator(ui: &mut egui::Ui) {
    if reduced_motion(ui.ctx()) {
        ui.label(egui::RichText::new("●").color(BLUE));
    } else {
        ui.spinner();
    }
}

pub const BG: Color32 = Color32::from_rgb(8, 11, 14);
pub const COLUMN: Color32 = Color32::from_rgb(13, 18, 23);
pub const PANEL: Color32 = Color32::from_rgb(19, 26, 32);
pub const PANEL_ALT: Color32 = Color32::from_rgb(24, 33, 41);
pub const CARD_HOVER: Color32 = Color32::from_rgb(29, 40, 49);
pub const BORDER: Color32 = Color32::from_rgb(45, 57, 66);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(60, 74, 84);
pub const TEXT: Color32 = Color32::from_rgb(244, 245, 242);
pub const TEXT_DIM: Color32 = Color32::from_rgb(167, 173, 180);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(120, 131, 141);
pub const PUNCH: Color32 = Color32::from_rgb(226, 29, 53);
pub const PUNCH_BRIGHT: Color32 = Color32::from_rgb(243, 55, 77);
pub const PUNCH_DEEP: Color32 = Color32::from_rgb(104, 10, 21);
pub const BLUE: Color32 = Color32::from_rgb(0, 159, 232);
pub const BLUE_BRIGHT: Color32 = Color32::from_rgb(34, 184, 255);
pub const ACCENT: Color32 = Color32::from_rgb(0, 159, 232);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(28, 78, 101);
/// At-a-glance digest backdrop on dark neutral surfaces.
pub const DIGEST_BG: Color32 = Color32::from_rgb(20, 50, 72);
/// Fill of the option-chip row, slightly brighter than the chat background.
pub const CHIP_FILL: Color32 = Color32::from_rgb(26, 42, 51);
/// Resting border of the option-chip row.
pub const CHIP_BORDER: Color32 = Color32::from_rgb(60, 83, 94);
pub const DANGER: Color32 = Color32::from_rgb(255, 83, 100);
pub const WARNING: Color32 = Color32::from_rgb(255, 194, 71);
pub const SUCCESS: Color32 = Color32::from_rgb(69, 212, 131);
pub const PURPLE: Color32 = Color32::from_rgb(182, 108, 255);
/// Assumption card hue (CHG-002 recorded default, overridable by a fresh
/// word); DANGER red stays deliberately unclaimed as a card hue (stip. ii).
pub const ASSUMPTION_LAVENDER: Color32 = Color32::from_rgb(34, 184, 255);
/// Ownership card hue (CHG-002 recorded default, overridable by a fresh
/// word); DANGER red stays deliberately unclaimed as a card hue (stip. ii).
pub const OWNERSHIP_PINK: Color32 = Color32::from_rgb(240, 106, 157);

/// Centralised class-to-card-hue mapping (CHG-002 recorded defaults):
/// Task-story (`None`) -> neutral, with approved colors for each planning type.
pub fn board_hue(kind: Option<crate::domain::ItemKind>) -> Color32 {
    use crate::domain::ItemKind::*;
    match kind {
        None => Color32::from_rgb(201, 209, 217),
        Some(Question) => PURPLE,
        Some(Ambiguity) => WARNING,
        Some(Assumption) => ASSUMPTION_LAVENDER,
        Some(Ownership) => OWNERSHIP_PINK,
    }
}

/// Card outlines identify item type; activity changes emphasis, not hue.
pub fn board_frame(kind: Option<crate::domain::ItemKind>, active: bool) -> egui::Frame {
    board_state_frame(kind, active, false, false)
}

pub fn board_state_frame(
    kind: Option<crate::domain::ItemKind>,
    active: bool,
    attention: bool,
    done: bool,
) -> egui::Frame {
    let width = if active || attention || done {
        1.5
    } else {
        1.0
    };
    let edge = board_hue(kind);
    egui::Frame::NONE
        .corner_radius(8)
        .inner_margin(14)
        .fill(if active { CARD_HOVER } else { PANEL_ALT })
        .stroke(Stroke::new(width, edge))
}

/// Base style applied once during creation.
pub fn koolade_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.window_fill = PANEL;
    v.panel_fill = BG;
    v.extreme_bg_color = BG;
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.fg_stroke.color = TEXT;
    v.widgets.noninteractive.corner_radius = CornerRadius::same(7);
    v.widgets.inactive.bg_fill = PANEL_ALT;
    v.widgets.inactive.fg_stroke.color = TEXT_DIM;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.hovered.bg_fill = CARD_HOVER;
    v.widgets.hovered.fg_stroke.color = TEXT;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    v.widgets.active.bg_fill = ACCENT_SOFT;
    v.widgets.active.fg_stroke.color = TEXT;
    v.selection.bg_fill = PUNCH_DEEP;
    v.selection.stroke = Stroke::new(1.0, PUNCH_BRIGHT);
    v.hyperlink_color = BLUE_BRIGHT;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.active.bg_stroke = Stroke::new(2.0, BLUE_BRIGHT);
    v.widgets.inactive.weak_bg_fill = PANEL_ALT;
    v.widgets.hovered.weak_bg_fill = BORDER;
    v.widgets.inactive.corner_radius = CornerRadius::same(7);
    v.widgets.hovered.corner_radius = CornerRadius::same(7);
    v.widgets.active.corner_radius = CornerRadius::same(7);
    v.widgets.open.corner_radius = CornerRadius::same(7);
    v
}

/// Rounded framed box helper (cards in panes).
pub fn card_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(PANEL)
        .corner_radius(8)
        .stroke(Stroke::new(1.0, BORDER))
        .inner_margin(egui::Margin::same(20))
}

/// Badge chip (priorities, kinds, statuses).
pub fn badge(ui: &mut egui::Ui, text: &str, bg: Color32, fg: Color32) -> egui::Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), egui::FontId::proportional(10.5), fg);
    let size = galley.size() + egui::vec2(10.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, 10.0, bg);
    ui.painter()
        .galley(rect.left_top() + egui::vec2(5.0, 3.0), galley, fg);
    resp
}

impl crate::domain::Priority {
    /// Chip color pairing for badges.
    pub fn badge_colors(&self) -> (Color32, Color32) {
        match self {
            crate::domain::Priority::Blocking => (Color32::from_rgb(80, 56, 20), WARNING),
            crate::domain::Priority::High => (Color32::from_rgb(80, 56, 20), WARNING),
            crate::domain::Priority::Normal => (Color32::from_rgb(11, 39, 54), ACCENT),
        }
    }
}

impl crate::domain::ItemKind {
    pub fn badge_colors(&self) -> (Color32, Color32) {
        match self {
            crate::domain::ItemKind::Question => (PURPLE_DARK, PURPLE),
            crate::domain::ItemKind::Ambiguity => (Color32::from_rgb(74, 52, 14), WARNING),
            crate::domain::ItemKind::Assumption => {
                (Color32::from_rgb(9, 48, 67), ASSUMPTION_LAVENDER)
            }
            crate::domain::ItemKind::Ownership => (Color32::from_rgb(74, 28, 47), OWNERSHIP_PINK),
        }
    }
}

const PURPLE_DARK: Color32 = Color32::from_rgb(44, 30, 73);

#[cfg(test)]
mod tests;
