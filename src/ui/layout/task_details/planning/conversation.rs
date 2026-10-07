use crate::ui::{ApplicationCommand, Surface, theme};
use egui::Ui;

pub(super) fn paint(ui: &mut Ui, surface: &mut dyn Surface, key: &str) {
    ui.heading("Task conversation");
    ui.label(theme::helper_text(
        "Messages and replies stay with this task.",
    ));
    let messages = surface.task_messages(key).to_vec();
    let busy = surface.task_chat_active(key);
    let context = surface.task_chat_context(key);
    if let Some(error) = surface.task_chat_error() {
        ui.colored_label(theme::WARNING, "Conversation has unsaved messages.");
        ui.label(theme::helper_text(error));
        if ui.button("Retry saving conversation").clicked() {
            surface.dispatch(ApplicationCommand::RetryTaskChatSave);
        }
    }
    let mut intent = crate::ui::chat_pane::Intent::default();
    if let Some(draft) = surface.task_draft(key) {
        intent = crate::ui::chat_pane::paint_task_with_context(
            ui,
            &messages,
            draft,
            busy,
            context.as_deref(),
        );
    } else {
        ui.label("Conversation is unavailable for this item.");
    }
    if intent.send {
        surface.dispatch(ApplicationCommand::SendTaskReply {
            key: key.to_owned(),
        });
    }
    if intent.cancel && busy {
        surface.dispatch(ApplicationCommand::CancelTaskReply {
            key: key.to_owned(),
        });
    }
}
