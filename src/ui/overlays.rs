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
    modal(ui, title, width, None, body)
}

/// Draw an X with strokes so missing font glyphs cannot turn it into a box.
pub fn close_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4, theme::PANEL_ALT);
    }
    let r = rect.shrink(9.0);
    let stroke = egui::Stroke::new(1.6, theme::TEXT);
    ui.painter()
        .line_segment([r.left_top(), r.right_bottom()], stroke);
    ui.painter()
        .line_segment([r.right_top(), r.left_bottom()], stroke);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close"));
    response
        .on_hover_text("Close")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn show_panel_modal(
    ui: &mut egui::Ui,
    title: &str,
    bounds: egui::Rect,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    modal(ui, title, bounds.width(), Some(bounds), body)
}

fn modal(
    ui: &mut egui::Ui,
    title: &str,
    width: f32,
    bounds: Option<egui::Rect>,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    let viewport = ui.ctx().content_rect();
    let bounds = bounds.map(|rect| rect.intersect(viewport).shrink(12.0));
    let width = bounds
        .map(|r| r.width() - 34.0)
        .unwrap_or(width.min(viewport.width() - 64.0))
        .max(80.0);
    let height = (bounds
        .map(|r| r.height())
        .unwrap_or(viewport.height() - 48.0)
        - 84.0)
        .max(40.0);
    let id = egui::Id::new("packet_modal").with(title);
    let mut modal = egui::Modal::new(id).frame(
        Frame::NONE
            .fill(theme::PANEL)
            .corner_radius(12.0)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .inner_margin(16),
    );
    if let Some(rect) = bounds {
        modal = modal.area(
            egui::Area::new(id)
                .kind(egui::UiKind::Modal)
                .order(egui::Order::Foreground)
                .fixed_pos(rect.left_top())
                .default_size(rect.size())
                .sense(egui::Sense::hover()),
        );
    }
    let mut closed = false;
    let response = modal.show(ui.ctx(), |ui| {
        ui.set_width(width);
        if bounds.is_some() {
            ui.set_height(height + 50.0);
        }
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.horizontal(|ui| {
            ui.add_sized(
                [(width - 38.0).max(20.0), 28.0],
                egui::Label::new(RichText::new(title).strong().size(17.0))
                    .halign(egui::Align::Min)
                    .truncate(),
            )
            .on_hover_text(title);
            closed = close_button(ui).clicked();
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt(("modal_body", title))
            .max_height(height)
            .auto_shrink([false, bounds.is_none()])
            .show(ui, |ui| {
                if bounds.is_some() {
                    ui.set_min_height(height);
                }
                body(ui);
            });
    });
    closed || response.should_close()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn item_modal_grows_when_the_workspace_is_resized() {
        let ctx = egui::Context::default();
        for size in [egui::vec2(1080.0, 640.0), egui::vec2(2560.0, 1440.0)] {
            let bounds = egui::Rect::from_min_max(
                egui::pos2(380.0, 90.0),
                egui::pos2(size.x - 28.0, size.y - 20.0),
            );
            for _ in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        ..Default::default()
                    },
                    |ui| {
                        show_panel_modal(ui, "Resize item", bounds, |ui| {
                            ui.label("Short content");
                        });
                    },
                );
                if let Some(rect) = output.shapes.iter().find_map(|s| match &s.shape {
                    egui::Shape::Rect(r) if r.corner_radius.nw == 12 && r.stroke.width == 1.0 => {
                        Some(r.rect)
                    }
                    _ => None,
                }) {
                    assert!(
                        rect.height() >= bounds.height() - 30.0,
                        "Modal failed to grow: {rect:?}"
                    );
                    assert!(bounds.contains_rect(rect), "Modal overflow: {rect:?}");
                }
                output.textures_delta.clear();
            }
        }
    }

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
