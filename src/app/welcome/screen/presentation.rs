//! Welcome branding uses the same artwork and palette as the workspace header.
use crate::ui::{layout::brand, theme};

pub(super) fn hero(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let height = if width < 380.0 { 140.0 } else { 200.0 };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 8, theme::PUNCH_DEEP);
    let splash = brand::splash(ui.ctx());
    let splash_width = width * 1.15;
    let splash_size = egui::vec2(
        splash_width,
        splash_width * splash.size_vec2().y / splash.size_vec2().x,
    );
    let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    painter.image(
        splash.id(),
        egui::Rect::from_center_size(rect.center(), splash_size),
        uv,
        egui::Color32::from_white_alpha(230),
    );
    let logo = brand::logo(ui.ctx(), false);
    let logo_width = (width * 0.6).min(320.0);
    let logo_size = egui::vec2(
        logo_width,
        logo_width * logo.size_vec2().y / logo.size_vec2().x,
    );
    painter.image(
        logo.id(),
        egui::Rect::from_center_size(rect.center(), logo_size),
        uv,
        egui::Color32::WHITE,
    );
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(2.0, theme::PUNCH_BRIGHT),
    );
}

/// Quiet section labels keep the two entry paths distinct.
pub(super) fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(text)
            .size(11.0)
            .strong()
            .color(theme::TEXT_DIM),
    );
    ui.add_space(2.0);
}
