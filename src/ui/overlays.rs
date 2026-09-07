//! Overlay primitives: a dimmed modal shell with title bar and ✕.
//! Closing rules (deliberately conservative to avoid stealing the opening
//! click or any button release happening inside the card):
//!   * Esc key anywhere
//!   * ✕ in the card's title row
//!   * pointer release ON the dim backdrop, off the card, at least 250 ms
//!     after the modal first appeared.

use egui::{Align2, Color32, Frame, Layout, Margin, Order, RichText};
use std::time::{Duration, Instant};

use crate::ui::theme;

/// Draw a modal if `open`. Returns TRUE when the user closed it (the
/// backdrop/Esc/✕ was hit), so callers can dispose their dialog.
pub fn show_modal<F>(
    ui: &mut egui::Ui,
    open: bool,
    title: &str,
    width: f32,
    body: F,
) -> bool
where
    F: FnOnce(&mut egui::Ui),
{
    if !open {
        BORN_AT.with(|b| b.set(None));
        return false;
    }
    let mut closed = false;
    let vp = ui.ctx().viewport_rect();

    // 1) Backdrop — full-viewport dim, rendered FIRST so the card (same
    //    order layer, shown later) stacks above it. Dismissal bookkeeping
    //    happens in step 3 once the card rect is known.
    {
        let mut esc = false;
        egui::Area::new(egui::Id::new("packet_modal_dim"))
            .order(Order::Foreground)
            .fixed_pos(vp.left_top())
            .interactable(true)
            .show(ui.ctx(), |ui| {
                ui.painter().rect_filled(vp, 0.0, Color32::from_black_alpha(140));
                esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        if esc {
            closed = true;
        }
    }

    // 2) Card — content-sized, height-clamped to the viewport, centered.
    let card_rect = egui::Area::new(egui::Id::new("packet_modal").with(title))
        .order(Order::Foreground)
        .anchor(Align2::CENTER_CENTER, [0.0, -10.0])
        .interactable(true)
        .show(ui.ctx(), |ui| {
            ui.set_min_width(width);
            // Never taller than the screen (minus breathing room); long
            // dialog bodies must opt into their own scrolling.
            ui.set_max_height((vp.height() - 48.0).max(240.0));
            Frame::NONE
                .fill(theme::PANEL)
                .corner_radius(10.0)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .inner_margin(Margin::symmetric(0, 14))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(20.0);
                        ui.label(
                            RichText::new(title)
                                .strong()
                                .size(15.0)
                                .color(theme::TEXT),
                        );
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(RichText::new("\u{2715}".to_string()).weak())
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                closed = true;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(10.0);
                    body(ui);
                    ui.add_space(12.0);
                });
        })
        .response
        .rect;

    // 3) Release-outside dismissal: pointer-down/up must both land off the
    //    card (releases the card swallowed never dismiss), after the
    //    anti-steal grace period.
    if !closed {
        let released_now =
            ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary));
        let over = ui
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|pos| card_rect.contains(pos));
        if released_now && ui.rect_contains_pointer(vp) && !over && backdrop_armed() {
            closed = true;
        }
    }

    closed
}

thread_local! {
    static BORN_AT: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// Grace-period gate: the backdrop only counts releases that arrive a beat
/// AFTER the modal first rendered (prevents the opening click from closing
/// it, and avoids closing on unrelated stale releases).
fn backdrop_armed() -> bool {
    let now = Instant::now();
    let arming = BORN_AT.with(|b| b.get().is_none());
    if arming {
        BORN_AT.with(|b| b.set(Some(now)));
        return false;
    }
    let age_ok = BORN_AT.with(|b| match b.get() {
        Some(born) => now.duration_since(born) >= Duration::from_millis(250),
        None => false,
    });
    age_ok
}
