//! Visual identity: a calm dark slate palette with restrained accents.
//! Everything in the UI draws its colors from here so the look stays coherent
//! (SPECIFICATION.md §22 "clean, professional visual identity").

use egui::{Color32, CornerRadius, Stroke};

pub const BG: Color32 = Color32::from_rgb(32, 32, 32);
pub const PANEL: Color32 = Color32::from_rgb(43, 43, 43);
pub const PANEL_ALT: Color32 = Color32::from_rgb(53, 53, 53);
pub const BORDER: Color32 = Color32::from_rgb(65, 65, 65);
pub const TEXT: Color32 = Color32::from_rgb(236, 234, 230);
pub const TEXT_DIM: Color32 = Color32::from_rgb(157, 157, 153);
pub const ACCENT: Color32 = Color32::from_rgb(218, 223, 212);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(66, 74, 64);
pub const DANGER: Color32 = Color32::from_rgb(255, 107, 107);
pub const WARNING: Color32 = Color32::from_rgb(255, 180, 84);
pub const SUCCESS: Color32 = Color32::from_rgb(107, 212, 144);
pub const PURPLE: Color32 = Color32::from_rgb(167, 139, 250);

/// Base style applied once during creation.
pub fn packet_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.window_fill = BG;
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = PANEL_ALT;
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.fg_stroke.color = TEXT;
    v.widgets.noninteractive.corner_radius = CornerRadius::same(8);
    v.widgets.inactive.bg_fill = PANEL_ALT;
    v.widgets.inactive.fg_stroke.color = TEXT_DIM;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.hovered.bg_fill = PANEL_ALT;
    v.widgets.hovered.fg_stroke.color = TEXT;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_SOFT);
    v.widgets.active.bg_fill = ACCENT_SOFT;
    v.widgets.active.fg_stroke.color = TEXT;
    v.selection.bg_fill = ACCENT_SOFT;
    v.hyperlink_color = ACCENT;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.weak_bg_fill = PANEL_ALT;
    v.widgets.hovered.weak_bg_fill = BORDER;
    v.widgets.inactive.corner_radius = CornerRadius::same(8);
    v.widgets.hovered.corner_radius = CornerRadius::same(8);
    v.widgets.active.corner_radius = CornerRadius::same(8);
    v
}

/// Rounded framed box helper (cards in panes).
pub fn card_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(PANEL)
        .corner_radius(14.0)
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
    ui.painter().rect_filled(rect, 4.0, bg);
    ui.painter()
        .galley(rect.left_top() + egui::vec2(5.0, 3.0), galley, fg);
    resp
}

impl crate::domain::Priority {
    /// Chip color pairing for badges.
    pub fn badge_colors(&self) -> (Color32, Color32) {
        match self {
            crate::domain::Priority::Blocking => (Color32::from_rgb(80, 26, 26), DANGER),
            crate::domain::Priority::High => (Color32::from_rgb(80, 56, 20), WARNING),
            crate::domain::Priority::Normal => (ACCENT_SOFT, ACCENT),
        }
    }
}

impl crate::domain::ItemKind {
    pub fn badge_colors(&self) -> (Color32, Color32) {
        match self {
            crate::domain::ItemKind::Question => (PURPLE_DARK, PURPLE),
            crate::domain::ItemKind::Ambiguity => (Color32::from_rgb(24, 66, 56), SUCCESS),
            crate::domain::ItemKind::Assumption => (
                Color32::from_rgb(56, 44, 80),
                Color32::from_rgb(190, 168, 255),
            ),
            crate::domain::ItemKind::Ownership => (
                Color32::from_rgb(74, 38, 62),
                Color32::from_rgb(232, 145, 190),
            ),
        }
    }
}

const PURPLE_DARK: Color32 = Color32::from_rgb(50, 40, 84);

#[cfg(test)]
mod tests {

    #[test]
    fn badge_impls_compile() {
        let (a, b) = crate::domain::Priority::High.badge_colors();
        let (c, d) = crate::domain::ItemKind::Question.badge_colors();
        let _ = (a, b, c, d);
    }
}
