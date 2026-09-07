//! Left pane: restored conversation (from ~/.packet), the working-status
//! strip while a turn runs, and the composer. Returns per-frame intents.

use egui::{Frame, Layout, RichText, TextEdit};

use crate::domain::chatlog::{ChatMessage, ChatRole};
use crate::ui::theme;

/// Per-frame intents produced by painting the pane.
#[derive(Default, Debug)]
pub struct Intent {
    pub send: bool,
    pub cancel: bool,
}

pub fn paint(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    draft: &mut String,
    busy: bool,
    activity: Option<&str>,
) -> Intent {
    let mut cancel = false;

    // ---------- message scroll ----------
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink(egui::Vec2b::new(false, false))
        .show(ui, |ui| {
            let max_w = ui.available_width();
            for m in messages {
                paint_message(ui, m, max_w);
                ui.add_space(10.0);
            }
        });

    // ---------- working strip (only while a turn runs) ----------
    if busy {
        Frame::NONE
            .fill(theme::PANEL_ALT)
            .corner_radius(6.0)
            .stroke(egui::Stroke::new(1.0, theme::ACCENT_SOFT))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("planner working…").weak());
                    if let Some(a) = activity {
                        ui.label(RichText::new(trunc(a, 56)).size(11.0).weak().monospace());
                    }
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("Cancel").weak()).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        ui.add_space(8.0);
    }

    // ---------- composer ----------
    let editable = !busy;
    ui.add_sized(
        egui::vec2(ui.available_width(), 74.0),
        TextEdit::multiline(draft)
            .hint_text("Describe requirements, raise questions, assign follow-ups…  (Ctrl+Enter sends)")
            .desired_width(f32::INFINITY)
            .desired_rows(3)
            .frame(egui::Frame::NONE.fill(egui::Color32::from_black_alpha(40)).corner_radius(6.0))
            .interactive(editable),
    );
    ui.add_space(6.0);
    let mut send = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(char_count(draft)).weak().size(11.0));
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            if editable {
                let blocked = draft.trim().is_empty();
                if ui
                    .add_enabled(
                        !blocked,
                        egui::Button::new(RichText::new("Send ➤").strong().color(theme::TEXT))
                            .frame(false),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    send = true;
                }
            }
        });
    });

    // Enter to send (guarded: non-empty draft AND likely-focused editor).
    let entered = ui.input(|i| i.key_pressed(egui::Key::Enter))
        && !ui.input(|i| i.modifiers.shift);
    if editable && entered && !draft.trim().is_empty() {
        // Heuristic focus check: the editor requested focus recently, or the
        // pointer is within the last allocated rect. Simpler robust rule —
        // send whenever Enter arrives with a non-blank draft: chat-first UX
        // favours responsiveness over accidental-proof.
        send = true;
    }

    Intent { send, cancel }
}

fn char_count(d: &str) -> String {
    format!("{}\u{00A0}chars", d.chars().count())
}

fn paint_message(ui: &mut egui::Ui, m: &ChatMessage, max_w: f32) {
    let mine = m.role == ChatRole::User;
    let border = if mine { theme::ACCENT_SOFT } else { theme::BORDER };
    let bg = if mine { theme::PANEL_ALT } else { theme::PANEL };
    let (who, who_color) = match m.role {
        ChatRole::User => ("you", theme::ACCENT),
        ChatRole::Agent => ("planner", theme::PURPLE),
        ChatRole::System => ("system", theme::TEXT_DIM),
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 7.0;
        ui.label(RichText::new(who).strong().size(11.0).color(who_color));
        ui.label(RichText::new(format!("{}", m.ts.format("%H:%M"))).weak().size(10.5).monospace());
        if let Some(ref_it) = &m.ref_item {
            theme::badge(ui, ref_it, theme::PANEL_ALT, theme::PURPLE);
        }
    });
    Frame::NONE
        .fill(bg)
        .corner_radius(6.0)
        .stroke(egui::Stroke::new(1.0, border))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_max_width((max_w - 20.0).max(120.0));
            for (li, line) in m.text.split('\n').enumerate() {
                if li > 0 {
                    ui.add_space(2.0);
                }
                ui.label(RichText::new(line).color(theme::TEXT));
            }
        });
}

fn trunc(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}
