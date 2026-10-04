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
    modal(ui, title, title, width, None, body)
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
    modal(ui, title, title, bounds.width(), Some(bounds), body)
}

pub fn show_task_modal(
    ui: &mut egui::Ui,
    ticket: &str,
    bounds: egui::Rect,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    modal(
        ui,
        "Task details",
        ticket,
        bounds.width(),
        Some(bounds),
        body,
    )
}

fn modal(
    ui: &mut egui::Ui,
    title: &str,
    identity: &str,
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
    let id = egui::Id::new("koolade_modal").with(identity);
    let mut modal = egui::Modal::new(id).frame(
        Frame::NONE
            .fill(theme::PANEL)
            .corner_radius(12.0)
            .stroke(egui::Stroke::new(1.5, theme::BLUE))
            .shadow(egui::epaint::Shadow {
                offset: [0, 8],
                blur: 28,
                spread: 2,
                color: egui::Color32::from_black_alpha(180),
            })
            .inner_margin(16),
    );
    let position_id = id.with("position");
    let saved_position = ui
        .ctx()
        .data_mut(|data| data.get_temp::<egui::Pos2>(position_id));
    let initial = bounds.map_or(viewport.center(), |rect| rect.left_top());
    let area = egui::Area::new(id)
        .kind(egui::UiKind::Modal)
        .order(egui::Order::Foreground)
        .pivot(if bounds.is_none() && saved_position.is_none() {
            egui::Align2::CENTER_CENTER
        } else {
            egui::Align2::LEFT_TOP
        })
        .fixed_pos(saved_position.unwrap_or(initial))
        .constrain_to(viewport)
        .default_size(egui::vec2(width + 34.0, height + 84.0));
    modal = modal.area(area);
    let mut drag = egui::Vec2::ZERO;
    let mut closed = false;
    let response = modal.show(ui.ctx(), |ui| {
        ui.set_width(width);
        if bounds.is_some() {
            ui.set_height(height + 50.0);
        }
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.horizontal(|ui| {
            let title_response = ui
                .add_sized(
                    [(width - 38.0).max(20.0), 28.0],
                    egui::Label::new(RichText::new(title).strong().size(17.0))
                        .halign(egui::Align::Min)
                        .sense(egui::Sense::drag())
                        .truncate(),
                )
                .on_hover_text(title);
            drag = title_response
                .on_hover_cursor(egui::CursorIcon::Grab)
                .drag_delta();
            closed = close_button(ui).clicked();
        });
        let (edge, _) = ui.allocate_exact_size(egui::vec2(width, 2.0), egui::Sense::hover());
        ui.painter().rect_filled(edge, 1, theme::PUNCH_BRIGHT);
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt(("modal_body", identity))
            .max_height(height)
            .auto_shrink([false, bounds.is_none()])
            .show(ui, |ui| {
                if bounds.is_some() {
                    ui.set_min_height(height);
                }
                body(ui);
            });
    });
    if drag != egui::Vec2::ZERO {
        let rect = response.response.rect;
        let desired = rect.min + drag;
        let position = egui::pos2(
            desired.x.clamp(
                viewport.left(),
                (viewport.right() - rect.width()).max(viewport.left()),
            ),
            desired.y.clamp(
                viewport.top(),
                (viewport.bottom() - rect.height()).max(viewport.top()),
            ),
        );
        ui.ctx()
            .data_mut(|data| data.insert_temp(position_id, position));
        ui.ctx().request_repaint();
    }
    closed || response.should_close()
}

#[cfg(test)]
mod tests;
