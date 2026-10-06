use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    ticket: &str,
    view: &mut crate::ui::task_detail::ViewModel,
    brief: Option<&crate::core::attention::Brief>,
    height: f32,
) {
    ui.heading("Task conversation");
    ui.label(
        RichText::new("Messages and replies stay with this task.")
            .small()
            .weak(),
    );
    let history_height = (height - 190.0).clamp(120.0, 420.0);
    egui::Frame::NONE
        .fill(theme::BG)
        .corner_radius(8)
        .inner_margin(8)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if view.messages.is_empty() {
                ui.label("No discussion yet.");
            } else {
                ui.push_id(("task_modal_conversation", ticket), |ui| {
                    crate::ui::task_chat::history_with_max_height(
                        ui,
                        &view.messages,
                        view.conversation_active,
                        history_height,
                    );
                });
            }
        });
    ui.add_space(8.0);
    let choices = brief
        .map(|brief| brief.options.as_slice())
        .unwrap_or_default();
    let decision_sent = !choices.is_empty()
        && view
            .messages
            .iter()
            .rev()
            .find(|message| message.role == crate::domain::ChatRole::User)
            .is_some_and(|message| message.text.starts_with("I choose option ("));
    if decision_sent {
        ui.collapsing("Change decision", |ui| {
            paint_reply(ui, s, ticket, view, choices)
        });
    } else {
        paint_reply(ui, s, ticket, view, choices);
    }
}

fn paint_reply(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    ticket: &str,
    view: &mut crate::ui::task_detail::ViewModel,
    choices: &[crate::core::attention::OptionBrief],
) {
    let old_draft = view.draft.clone();
    let outcome = reply::paint(
        ui,
        ticket,
        &mut view.draft,
        view.conversation_active,
        &view.messages,
        choices,
        view.conversation_error.as_deref(),
    );
    if outcome.retry_save {
        s.dispatch(crate::ui::ApplicationCommand::TaskDetail(
            crate::ui::task_detail::Command::RetryChatSave,
        ));
    }
    if let Some(decision) = outcome.submit_decision {
        s.dispatch(crate::ui::ApplicationCommand::TaskDetail(
            crate::ui::task_detail::Command::SubmitReply {
                ticket: ticket.to_owned(),
                draft: view.draft.clone(),
                decision,
            },
        ));
    } else if view.draft != old_draft {
        s.dispatch(crate::ui::ApplicationCommand::TaskDetail(
            crate::ui::task_detail::Command::UpdateDraft {
                ticket: ticket.to_owned(),
                draft: view.draft.clone(),
            },
        ));
    }
}
