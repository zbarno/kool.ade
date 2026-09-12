//! Shared viewport-bounded modal with an input-blocking backdrop.
use crate::ui::theme;
use egui::{Frame, RichText};

pub fn show_modal<F>(ui: &mut egui::Ui, open: bool, title: &str, width: f32, body: F) -> bool
where
    F: FnOnce(&mut egui::Ui),
{
    if !open {
        return false;
    }
    let viewport = ui.ctx().content_rect();
    let width = width.min((viewport.width() - 64.0).max(80.0));
    let height = (viewport.height() - 116.0).max(40.0);
    let mut closed = false;
    let response = egui::Modal::new(egui::Id::new("packet_modal").with(title))
        .frame(
            Frame::NONE
                .fill(theme::PANEL)
                .corner_radius(12.0)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .inner_margin(16),
        )
        .show(ui.ctx(), |ui| {
            ui.set_width(width);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.horizontal(|ui| {
                ui.add_sized(
                    [(width - 38.0).max(20.0), 24.0],
                    egui::Label::new(RichText::new(title).strong().size(17.0)).truncate(),
                )
                .on_hover_text(title);
                closed = ui.button("✕").on_hover_text("Close").clicked();
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt(("modal_body", title))
                .max_height(height)
                .auto_shrink([false, true])
                .show(ui, body);
        });
    closed || response.should_close()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_modal_stays_inside_small_and_large_viewports_and_closes_with_escape() {
        for size in [
            egui::vec2(420.0, 320.0),
            egui::vec2(1080.0, 640.0),
            egui::vec2(1480.0, 900.0),
        ] {
            let ctx = egui::Context::default();
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for step in 0..4 {
                let mut closed = false;
                let events = if step == 3 {
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    }]
                } else {
                    vec![]
                };
                let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(viewport), events, ..Default::default() }, |ui| {
                    closed = show_modal(ui, true, "Large dialog", 860.0, |ui| {
                        for _ in 0..50 { ui.label("Long content that needs to wrap and scroll within this dialog."); }
                    });
                });
                if step >= 2 {
                    fn panel(shape: &egui::Shape) -> Option<egui::Rect> {
                        match shape {
                            egui::Shape::Rect(rect)
                                if rect.corner_radius.nw == 12 && rect.stroke.width == 1.0 =>
                            {
                                Some(rect.rect)
                            }
                            egui::Shape::Vec(shapes) => shapes.iter().find_map(panel),
                            _ => None,
                        }
                    }
                    let card = output
                        .shapes
                        .iter()
                        .find_map(|shape| panel(&shape.shape))
                        .expect("modal frame");
                    assert!(viewport.contains_rect(card), "{size:?}: {card:?}");
                }
                assert_eq!(closed, step == 3);
                output.textures_delta.clear();
            }
        }
    }
}
