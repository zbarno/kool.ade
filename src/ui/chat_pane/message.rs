use crate::domain::chatlog::{ChatMessage, ChatRole};
use crate::ui::theme;
use egui::{Frame, RichText};
pub(super) fn paint_message(
    ui: &mut egui::Ui,
    m: &ChatMessage,
    max_w: f32,
    lift: bool,
    draft: &mut String,
    chip_fired: &mut bool,
    interactive: bool,
) {
    let mine = m.role == ChatRole::User;
    let indent = if mine { 32.0 } else { 0.0 };
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.vertical(|ui| {
            ui.set_width((max_w - indent - 8.0).max(100.0));
            if !mine {
                ui.horizontal(|ui| {
                    if m.role != ChatRole::Agent {
                        ui.label(RichText::new("Update").size(11.0).weak());
                    }
                    ui.label(
                        RichText::new(m.ts.format("%H:%M").to_string())
                            .size(10.0)
                            .weak(),
                    );
                    if let Some(item) = &m.ref_item {
                        theme::badge(ui, item, theme::PANEL_ALT, theme::TEXT_DIM);
                    }
                });
                ui.add_space(2.0);
            }
            Frame::NONE
                .fill(if mine {
                    theme::PANEL_ALT
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(20.0)
                .inner_margin(if mine { 16 } else { 0 })
                .show(ui, |ui| {
                    if m.role == ChatRole::Agent {
                        // Agent prose renders as structured dark-theme
                        // Markdown. Card-tab messages arrive pre-shielded by
                        // layout.rs and `readable` is idempotent on
                        // already-shielded prose, so the re-shield composes.
                        let shielded = crate::ui::message_text::readable(m);
                        if lift {
                            // Lifted reply: the tail leaves the body (it no
                            // longer sits buried in the text) and paints as
                            // its own distinct block directly underneath.
                            let tail = crate::ui::reply_tail::parse_reply_tail(&shielded);
                            crate::ui::markdown::paint(ui, &tail.body, crate::ui::markdown::CHAT);
                            crate::ui::reply_tail::paint_open_ask(ui, &tail);
                            // CHG-003 story 5: tappable chips for digest
                            // bullets that carry recognisable options
                            // (2..=6 matches, else the tail declines; a tap
                            // joins the FULL choice text, never sends).
                            let choices = crate::ui::reply_tail::digest_choices(&tail);
                            if let Some(index) =
                                crate::ui::reply_tail::paint_chip_row(ui, &choices, interactive)
                            {
                                crate::ui::reply_tail::join_choice(draft, &choices[index], '\n');
                                *chip_fired = true;
                            }
                        } else {
                            crate::ui::markdown::paint(ui, &shielded, crate::ui::markdown::CHAT);
                        }
                    } else {
                        // User bubbles and System notices stay plain —
                        // only agent prose is Markdown-rendered.
                        ui.add(
                            egui::Label::new(
                                RichText::new(m.text.trim())
                                    .size(15.0)
                                    .line_height(Some(23.0))
                                    .color(theme::TEXT),
                            )
                            .wrap(),
                        );
                    }
                });
            if m.role == ChatRole::Agent {
                let readable = crate::ui::message_text::readable(m);
                if readable.as_ref() != m.text {
                    ui.push_id(&m.id, |ui| {
                        ui.collapsing("Response details", |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(&m.text).monospace().small()).wrap(),
                            );
                        });
                    });
                }
            }
        });
    });
}
