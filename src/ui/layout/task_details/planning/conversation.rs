use crate::ui::{ApplicationCommand, Surface, theme};
use egui::Ui;

pub(super) fn paint(ui: &mut Ui, surface: &mut dyn Surface, key: &str) {
    paint_with_focus(ui, surface, key, false);
}

pub(super) fn paint_with_focus(
    ui: &mut Ui,
    surface: &mut dyn Surface,
    key: &str,
    focus_editor: bool,
) {
    ui.heading("Ask a question");
    ui.label(theme::helper_text("Get a quick answer from Kool.ad/e."));
    ui.add_space(20.0);
    let messages = surface.task_messages(key).to_vec();
    let busy = surface.task_chat_active(key);
    let error = surface.task_chat_error().map(str::to_owned);
    let height = (ui.available_height() - 140.0).max(120.0);
    ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
        ui.set_min_height(height);
        super::super::conversation::paint_history(ui, &messages, busy, height);
    });
    let _ = focus_editor;
    let Some(draft) = surface.task_draft(key) else {
        ui.label("Conversation is unavailable for this item.");
        return;
    };
    let outcome =
        super::super::reply::paint(ui, key, draft, busy, &messages, &[], error.as_deref());
    let question = draft.clone();
    if outcome.submit_decision.is_some() {
        surface.dispatch(ApplicationCommand::TaskDetail(
            crate::ui::task_detail::Command::AskQuestion {
                ticket: key.to_owned(),
                draft: question,
            },
        ));
    }
    if outcome.retry_save {
        surface.dispatch(ApplicationCommand::RetryTaskChatSave);
    }
}
