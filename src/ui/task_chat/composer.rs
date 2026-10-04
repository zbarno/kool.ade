use crate::ui::{ApplicationCommand, Surface, theme};
use egui::RichText;
pub(super) fn composer(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    key: &str,
    expanded: bool,
    answer: bool,
    chip_fired: bool,
) {
    let busy = s.task_chat_active(key);
    let mut send = false;
    if let Some(draft) = s.task_draft(key) {
        let salt = if answer {
            "task_answer_composer"
        } else {
            "task_context_composer"
        };
        let response = if expanded {
            egui::ScrollArea::vertical()
                .id_salt((key, salt, "draft_scroll"))
                .max_height(88.0)
                .show(ui, |ui| {
                    egui::TextEdit::multiline(draft)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .hint_text(if answer {
                            "Your answer…"
                        } else {
                            "Add a follow-up…"
                        })
                        .id_salt(salt)
                        .show(ui)
                })
                .inner
        } else {
            egui::TextEdit::singleline(draft)
                .desired_width(f32::INFINITY)
                .hint_text(if answer {
                    "Your answer…"
                } else {
                    "Add a follow-up…"
                })
                .id_salt(salt)
                .show(ui)
        };
        if chip_fired {
            // A chip tap landed this frame: park the caret behind the
            // freshly joined option and hand the box back.
            crate::ui::reply_tail::pin_caret_to_end(&response, ui.ctx(), draft);
        }
        let enter = if expanded {
            response.response.has_focus()
                && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter))
        } else {
            response.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        };
        let enabled = !busy && !draft.trim().is_empty();
        let button = egui::Button::new(
            RichText::new(if answer { "Send answer" } else { "Send reply" }).strong(),
        )
        .fill(theme::ACCENT_SOFT);
        send = ui
            .add_enabled(enabled, button)
            .on_hover_text(if busy {
                "Another update is running. Your draft stays here until you can send it."
            } else if expanded {
                "Send to this task only · Ctrl / ⌘ + Enter"
            } else {
                "Send to this task only · Enter"
            })
            .clicked()
            || (enabled && enter);
        if expanded {
            ui.label(
                RichText::new(if busy {
                    "Waiting for the current update. Your draft is kept."
                } else {
                    "Ctrl / ⌘ + Enter to send"
                })
                .small()
                .weak(),
            );
        }
    }
    if send {
        s.dispatch(ApplicationCommand::SendTaskReply {
            key: key.to_owned(),
        });
    }
}
