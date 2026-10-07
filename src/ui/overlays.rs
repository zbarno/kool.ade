//! Shared viewport-bounded modal with an input-blocking backdrop.
use crate::ui::{
    surface::{SurfaceBounds, SurfaceKind},
    theme,
};
use egui::{Frame, RichText};

type ModalFooter<'a> = Option<Box<dyn FnOnce(&mut egui::Ui) + 'a>>;

struct ModalConfig<'a> {
    title: &'a str,
    identity: &'a str,
    width: f32,
    kind: SurfaceKind,
    bounds: Option<egui::Rect>,
    footer: ModalFooter<'a>,
    body_owns_scroll: bool,
}

pub fn show_modal<F>(ui: &mut egui::Ui, open: bool, title: &str, width: f32, body: F) -> bool
where
    F: FnOnce(&mut egui::Ui),
{
    if !open {
        return false;
    }
    let kind = if width >= 700.0 {
        SurfaceKind::Medium
    } else {
        SurfaceKind::Small
    };
    modal(
        ui,
        ModalConfig {
            title,
            identity: title,
            width,
            kind,
            bounds: None,
            footer: None,
            body_owns_scroll: false,
        },
        body,
    )
}

pub fn show_medium_modal<B, F>(
    ui: &mut egui::Ui,
    open: bool,
    title: &str,
    body: B,
    footer: F,
) -> bool
where
    B: FnOnce(&mut egui::Ui),
    F: FnOnce(&mut egui::Ui),
{
    if !open {
        return false;
    }
    modal(
        ui,
        ModalConfig {
            title,
            identity: title,
            width: 800.0,
            kind: SurfaceKind::Medium,
            bounds: None,
            footer: Some(Box::new(footer)),
            body_owns_scroll: false,
        },
        body,
    )
}

pub fn show_workspace_modal(
    ui: &mut egui::Ui,
    open: bool,
    title: &str,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    if !open {
        return false;
    }
    modal(
        ui,
        ModalConfig {
            title,
            identity: title,
            width: 0.0,
            kind: SurfaceKind::Workspace,
            bounds: None,
            footer: None,
            body_owns_scroll: false,
        },
        body,
    )
}

/// Settings use a readable content width even on very large desktops.
pub fn show_settings_modal(
    ui: &mut egui::Ui,
    open: bool,
    title: &str,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    if !open {
        return false;
    }
    modal(
        ui,
        ModalConfig {
            title,
            identity: title,
            width: 1120.0,
            kind: SurfaceKind::Workspace,
            bounds: None,
            footer: None,
            body_owns_scroll: true,
        },
        body,
    )
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
    modal(
        ui,
        ModalConfig {
            title,
            identity: title,
            width: bounds.width(),
            kind: SurfaceKind::Workspace,
            bounds: Some(bounds),
            footer: None,
            body_owns_scroll: false,
        },
        body,
    )
}

pub fn show_task_modal(
    ui: &mut egui::Ui,
    ticket: &str,
    bounds: egui::Rect,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    modal(
        ui,
        ModalConfig {
            title: "Task details",
            identity: ticket,
            width: bounds.width(),
            kind: SurfaceKind::Workspace,
            bounds: None,
            footer: None,
            body_owns_scroll: false,
        },
        body,
    )
}

fn modal(ui: &mut egui::Ui, config: ModalConfig<'_>, body: impl FnOnce(&mut egui::Ui)) -> bool {
    let ModalConfig {
        title,
        identity,
        width,
        kind,
        bounds,
        footer,
        body_owns_scroll,
    } = config;
    let viewport = ui.ctx().content_rect();
    let metrics = SurfaceBounds::for_viewport(viewport, kind, width);
    let bounds = bounds.map(|rect| rect.intersect(viewport).shrink(8.0));
    let rect = bounds.unwrap_or(metrics.rect);
    let width = (rect.width() - 34.0).max(80.0);
    let footer_height = if footer.is_some() { 62.0 } else { 0.0 };
    let height = (rect.height() - 84.0 - footer_height).max(40.0);
    let id = egui::Id::new("koolade_modal").with(identity);
    let mut modal = egui::Modal::new(id).frame(
        Frame::NONE
            .fill(theme::PANEL)
            .corner_radius(12.0)
            .stroke(egui::Stroke::new(1.5, theme::BORDER_STRONG))
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
    let initial = rect.left_top();
    let area = egui::Area::new(id)
        .kind(egui::UiKind::Modal)
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::LEFT_TOP)
        .fixed_pos(saved_position.unwrap_or(initial))
        .constrain_to(viewport)
        .default_size(egui::vec2(width + 34.0, height + 84.0 + footer_height));
    modal = modal.area(area);
    let mut drag = egui::Vec2::ZERO;
    let mut closed = false;
    let response = modal.show(ui.ctx(), |ui| {
        ui.set_width(width);
        if kind == SurfaceKind::Workspace {
            ui.set_height(height + 50.0);
        } else if footer_height > 0.0 {
            ui.set_height(height + 52.0 + footer_height);
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
            closed = close_button(ui).clicked()
                || ui.input(|input| input.key_pressed(egui::Key::Escape));
        });
        let (edge, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
        ui.painter().rect_filled(edge, 0, theme::BORDER);
        ui.add_space(6.0);
        if body_owns_scroll {
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                body,
            );
        } else {
            egui::ScrollArea::vertical()
                .id_salt(("modal_body", identity))
                .max_height(height)
                .auto_shrink([false, kind != SurfaceKind::Workspace])
                .show(ui, |ui| {
                    if kind == SurfaceKind::Workspace {
                        ui.set_min_height(height);
                    }
                    body(ui);
                });
        }
        if let Some(footer) = footer {
            ui.add_space(theme::spacing::S);
            ui.separator();
            ui.add_space(theme::spacing::XS);
            footer(ui);
        }
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
