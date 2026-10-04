use crate::domain::ChatMessage;
use crate::ui::task_chat::composer::composer;
use crate::ui::task_chat::transcript::transcript;
use crate::ui::{ApplicationCommand, Surface, theme};
use egui::RichText;

#[allow(clippy::too_many_arguments)] // renders the task conversation footer from its view state
pub(super) fn paint_footer(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    key: &str,
    expanded: bool,
    active: bool,
    review: bool,
    needs_answer: bool,
    retry: bool,
    blocked: bool,
    messages: &[ChatMessage],
    summary: &str,
    open: &mut bool,
) {
    if !expanded && !summary.is_empty() && !active {
        ui.label(RichText::new(crate::core::context_build::clip(summary, 120)).small())
            .on_hover_text("Open this task for the complete conversation.");
    }
    if let Some(error) = s.task_chat_error() {
        ui.colored_label(theme::WARNING, "Conversation has unsaved messages.");
        ui.label(RichText::new(error).small());
        if ui.button("Retry saving conversation").clicked() {
            s.dispatch(ApplicationCommand::RetryTaskChatSave);
        }
    }
    // A compact Kanban card only owns an input while Koolade is explicitly
    // asking for one. Free-form context remains available after opening
    // the task's full conversation.
    if expanded && review {
        ui.label(
            RichText::new("Suggest a different choice or add context")
                .small()
                .strong(),
        );
        composer(ui, s, key, expanded, false, false);
    }
    if expanded && !needs_answer && !retry && !review && !blocked {
        let has_draft = s.task_draft(key).is_some_and(|draft| !draft.is_empty());
        egui::CollapsingHeader::new("Add context")
            .default_open(has_draft)
            .open(has_draft.then_some(true))
            .show(ui, |ui| composer(ui, s, key, expanded, false, false));
    }
    if expanded && !messages.is_empty() {
        ui.collapsing(format!("Conversation history ({})", messages.len()), |ui| {
            transcript(ui, messages, active)
        });
    } else if !expanded && !review {
        ui.horizontal_wrapped(|ui| {
        if ui.small_button("Open conversation").on_hover_text("Continue this same conversation with the task context and full history. Your draft comes with you.").clicked() {
            *open = true;
        }
        if !messages.is_empty() {
            ui.label(RichText::new(format!("{} messages", messages.len())).small().weak());
        }
    });
    }
}
