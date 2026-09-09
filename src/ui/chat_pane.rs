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
    pub generate_tasks: bool,
}

pub fn paint(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    draft: &mut String,
    busy: bool,
    progress: Option<&crate::harness::LiveProgress>,
    offer: Option<&crate::core::workflow::InterviewBrief>,
) -> Intent {
    let mut cancel = false;
    let mut generate_tasks = false;

    // ---------- message scroll ----------
    // RESERVE composer + (optionally) working-strip height UP FRONT:
    // with the messages list unconstrained, an empty conversation would eat
    // the whole pane and push the composer below the fold (invisible box).
    let reserve: f32 = 130.0 + if busy { 52.0 } else { 0.0 };
    ui.scope(|ui| {
        ui.set_max_height((ui.available_height() - reserve).max(48.0));
        egui::ScrollArea::vertical()
            .id_salt("conversation_scroll")
            .stick_to_bottom(true)
            .auto_shrink(egui::Vec2b::new(false, false))
            .show(ui, |ui| {
                let max_w = ui.available_width();
                for m in messages {
                    paint_message(ui, m, max_w);
                    ui.add_space(10.0);
                }
                if let Some(brief) = offer {
                    theme::card_frame().show(ui, |ui| {
                        ui.label(RichText::new("Ready for task stories").strong());
                        ui.label(RichText::new(&brief.feature_name).size(13.0));
                        ui.label(RichText::new("Turn the agreed scope into a detailed implementation plan, or keep refining it below.").size(12.0).weak());
                        generate_tasks = ui.button("Generate task stories").clicked();
                    });
                }
                if let Some(progress) = progress {
                    paint_progress(ui, progress);
                }
            });
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
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("Cancel").weak()).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        ui.add_space(8.0);
    }

    // Keep the editor and its actions in one rounded surface.
    let editable = !busy;
    let mut send = false;
    Frame::NONE
        .fill(theme::PANEL_ALT)
        .corner_radius(22.0)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            let editor = ui.add_sized(
                egui::vec2(ui.available_width(), 48.0),
                TextEdit::multiline(draft)
                    .hint_text("What are you building?")
                    .desired_width(f32::INFINITY)
                    .desired_rows(2)
                    .frame(egui::Frame::NONE)
                    .interactive(editable),
            );
            ui.horizontal(|ui| {
                ui.label(RichText::new("Ctrl + Enter to send").size(10.5).weak());
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    let enabled = editable && !draft.trim().is_empty();
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
                    let center = rect.center();
                    ui.painter().circle_filled(
                        center,
                        18.0,
                        if enabled {
                            theme::TEXT
                        } else {
                            theme::TEXT_DIM
                        },
                    );
                    let stroke = egui::Stroke::new(2.0, theme::BG);
                    ui.painter().line_segment(
                        [center + egui::vec2(0.0, 7.0), center - egui::vec2(0.0, 7.0)],
                        stroke,
                    );
                    ui.painter().line_segment(
                        [
                            center + egui::vec2(-6.0, -1.0),
                            center - egui::vec2(0.0, 7.0),
                        ],
                        stroke,
                    );
                    ui.painter().line_segment(
                        [
                            center + egui::vec2(6.0, -1.0),
                            center - egui::vec2(0.0, 7.0),
                        ],
                        stroke,
                    );
                    send = response.on_hover_text("Send message").clicked() && enabled;
                });
            });
            if editable
                && editor.has_focus()
                && !draft.trim().is_empty()
                && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter))
            {
                send = true;
            }
        });
    Intent {
        send,
        cancel,
        generate_tasks,
    }
}

fn paint_message(ui: &mut egui::Ui, m: &ChatMessage, max_w: f32) {
    let mine = m.role == ChatRole::User;
    let indent = if mine { 32.0 } else { 0.0 };
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.vertical(|ui| {
            ui.set_width((max_w - indent - 8.0).max(100.0));
            if !mine {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if m.role == ChatRole::Agent {
                            "Packet"
                        } else {
                            "Update"
                        })
                        .size(12.0)
                        .strong(),
                    );
                    ui.label(
                        RichText::new(m.ts.format("%H:%M").to_string())
                            .size(10.0)
                            .weak(),
                    );
                    if let Some(item) = &m.ref_item {
                        theme::badge(ui, item, theme::PANEL_ALT, theme::TEXT_DIM);
                    }
                });
                ui.add_space(6.0);
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
                    ui.add(
                        egui::Label::new(
                            RichText::new(&m.text)
                                .size(15.0)
                                .line_height(Some(23.0))
                                .color(theme::TEXT),
                        )
                        .wrap(),
                    );
                });
        });
    });
    ui.add_space(12.0);
}

fn paint_progress(ui: &mut egui::Ui, progress: &crate::harness::LiveProgress) {
    ui.push_id("live_turn", |ui| {
        if !progress.posts.is_empty() {
            for post in &progress.posts {
                ui.push_id(post.id, |ui| {
                    if post.kind == "thinking" {
                        egui::CollapsingHeader::new(
                            RichText::new("Thinking").size(12.0).color(theme::TEXT_DIM),
                        )
                        .id_salt("thought_block")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&post.text)
                                        .size(13.0)
                                        .line_height(Some(20.0))
                                        .color(theme::TEXT_DIM),
                                )
                                .wrap(),
                            );
                        });
                    } else {
                        ui.label(RichText::new("Packet").size(12.0).strong());
                        ui.add(
                            egui::Label::new(
                                RichText::new(&post.text)
                                    .size(15.0)
                                    .line_height(Some(23.0))
                                    .color(theme::TEXT),
                            )
                            .wrap(),
                        );
                    }
                    ui.add_space(12.0);
                });
            }
            if let Some(activity) = &progress.activity {
                ui.label(RichText::new(activity).size(11.0).weak());
            }
            return;
        }

        if !progress.thoughts.is_empty() {
            egui::CollapsingHeader::new(
                RichText::new("Thinking").size(12.0).color(theme::TEXT_DIM),
            )
            .default_open(true)
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(&progress.thoughts)
                            .size(13.0)
                            .line_height(Some(20.0))
                            .color(theme::TEXT_DIM),
                    )
                    .wrap(),
                );
            });
            ui.add_space(12.0);
        }
        if let Some(activity) = &progress.activity {
            ui.add(
                egui::Label::new(RichText::new(activity).size(11.0).color(theme::TEXT_DIM)).wrap(),
            );
            ui.add_space(12.0);
        }
        if !progress.response.is_empty() {
            ui.label(RichText::new("Packet").size(12.0).strong());
            ui.add(
                egui::Label::new(
                    RichText::new(&progress.response)
                        .size(15.0)
                        .line_height(Some(23.0))
                        .color(theme::TEXT),
                )
                .wrap(),
            );
            ui.add_space(12.0);
        }
    });
}
