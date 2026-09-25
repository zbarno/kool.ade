//! Inline task replies and complete state messages.
use super::*;
use crate::domain::{ChatMessage, ChatRole};

pub(super) fn full_message(ui: &mut egui::Ui, message: &str, id: &str) {
    let height = if ui.ctx().content_rect().width() < 600.0 {
        110.0
    } else {
        180.0
    };
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(height)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            crate::ui::markdown::paint(ui, message, crate::ui::markdown::CHAT);
        });
    if ui.small_button("Copy full message").clicked() {
        ui.ctx().copy_text(message.to_owned());
    }
}

pub(super) fn failure_actions(text: &str) -> Vec<&str> {
    text.split_once("### Next action(s)")
        .map(|(_, tail)| {
            tail.lines()
                .filter_map(|line| line.trim().strip_prefix("- "))
                .collect()
        })
        .unwrap_or_default()
}

fn paint_blocker_choice(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    key: &str,
    busy: bool,
    choice: &crate::core::attention::OptionBrief,
) {
    egui::Frame::NONE
        .fill(theme::PANEL_ALT)
        .corner_radius(6)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(
                        RichText::new(format!("{} · {}", choice.id, choice.label)).strong(),
                    )
                    .wrap(),
                )
                .on_hover_text(&choice.meaning)
                .clicked()
                && let Some(draft) = s.task_draft(key)
            {
                *draft = format!("I choose option ({}): {}.", choice.id, choice.label);
            }
            ui.add(egui::Label::new(&choice.meaning).wrap());
            ui.add(
                egui::Label::new(
                    RichText::new(format!("If chosen: {}", choice.consequence))
                        .small()
                        .color(theme::TEXT_DIM),
                )
                .wrap(),
            );
        });
    ui.add_space(6.0);
}

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    key: &str,
    messages: &[ChatMessage],
    blocker_choices: &[crate::core::attention::OptionBrief],
) {
    let choices = if blocker_choices.is_empty() {
        crate::ui::reply_tail::open_digest_choices(messages)
    } else {
        Vec::new()
    };
    let busy = s.task_chat_active(key);
    ui.push_id(("task_detail_reply", key), |ui| {
        if !blocker_choices.is_empty() {
            ui.label(RichText::new("Choose one option").strong());
            if ui.available_width() >= 660.0 {
                ui.columns(2, |columns| {
                    for (index, choice) in blocker_choices.iter().enumerate() {
                        paint_blocker_choice(&mut columns[index % 2], s, key, busy, choice);
                    }
                });
            } else {
                for choice in blocker_choices {
                    paint_blocker_choice(ui, s, key, busy, choice);
                }
            }
        } else if !choices.is_empty() {
            ui.label(RichText::new("Choose an option").small().strong());
            ui.horizontal_wrapped(|ui| {
                for choice in &choices {
                    if ui
                        .add_enabled(!busy, egui::Button::new(&choice.text).wrap())
                        .clicked()
                        && let Some(draft) = s.task_draft(key)
                    {
                        crate::ui::reply_tail::join_choice(draft, choice, '\n');
                    }
                }
            });
        }
        ui.label(
            RichText::new(if !blocker_choices.is_empty() {
                "Review or explain your decision"
            } else if choices.is_empty() {
                "Reply to this task"
            } else {
                "Add detail or edit your answer"
            })
            .small()
            .strong(),
        );
        let mut send = false;
        if let Some(draft) = s.task_draft(key) {
            let response = egui::TextEdit::multiline(draft)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("Your response…")
                .id_salt("task_detail_reply_text")
                .show(ui);
            let enabled = !busy && !draft.trim().is_empty();
            send = ui
                .add_enabled(
                    enabled,
                    egui::Button::new(if blocker_choices.is_empty() {
                        "Send response"
                    } else {
                        "Send decision"
                    }),
                )
                .on_hover_text("Send to this task's conversation")
                .clicked()
                || (enabled
                    && response.response.has_focus()
                    && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)));
        }
        if send {
            if blocker_choices.is_empty() {
                s.send_task_reply(key);
            } else {
                s.send_implementation_decision(key);
            }
        }
        if busy {
            ui.label(
                RichText::new("Packet is responding. Your draft is kept.")
                    .small()
                    .weak(),
            );
        }
        if messages.last().is_some_and(|m| m.role == ChatRole::User) {
            ui.label(
                RichText::new(if blocker_choices.is_empty() {
                    "Your response is saved in this task's conversation."
                } else {
                    "Decision saved. Resume after the remaining steps are complete."
                })
                .small()
                .weak(),
            );
        }
        if let Some(error) = s.task_chat_error() {
            ui.colored_label(theme::WARNING, error);
            if ui.small_button("Retry saving response").clicked() {
                s.retry_task_chat_save();
            }
        }
    });
}
