use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    task: &crate::persistence::setup_tasks::LocalTask,
) {
    let conversation = surface.bootstrap_conversation(&task.id);
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("Setup assistant"));
    ui.label(crate::ui::theme::helper_text(
        "Uses only this machine setup summary and this conversation. It cannot inspect a workspace or run tools.",
    ));
    egui::ScrollArea::vertical()
        .id_salt(("bootstrap-conversation-history", &task.id))
        .max_height(260.0)
        .show(ui, |ui| {
            if conversation.messages.is_empty() {
                ui.label(crate::ui::theme::helper_text(
                    "Ask a question about this setup step or request a safe next step.",
                ));
            }
            for message in &conversation.messages {
                ui.group(|ui| {
                    let speaker = match message.role {
                        crate::domain::chatlog::ChatRole::User => "You",
                        crate::domain::chatlog::ChatRole::Agent => "Assistant",
                        crate::domain::chatlog::ChatRole::System => "Setup status",
                    };
                    ui.label(crate::ui::theme::metadata_text(speaker));
                    ui.add(egui::Label::new(&message.text).wrap());
                });
            }
        });
    if let Some(error) = conversation.error.as_deref() {
        ui.colored_label(crate::ui::theme::WARNING, error);
    }
    if let Some(suggestion) = &conversation.suggestion {
        let (title, action) = match &suggestion.operation {
            crate::ui::planning_board::BootstrapSuggestionOperation::RecheckPrerequisites => {
                ("Recheck this machine", "Review and recheck")
            }
            crate::ui::planning_board::BootstrapSuggestionOperation::PrepareLockedImage {
                profile,
            } => (
                if profile == "general" {
                    "Prepare the general runtime image"
                } else {
                    "Prepare the suggested runtime image"
                },
                "Review image preparation",
            ),
            crate::ui::planning_board::BootstrapSuggestionOperation::OpenCodingTools => {
                ("Review Coding Tools settings", "Open Coding Tools")
            }
        };
        ui.group(|ui| {
            ui.label(crate::ui::theme::section_heading("Suggested next step"));
            ui.label(title);
            ui.label(crate::ui::theme::helper_text(
                "Nothing runs automatically. Review the action, then approve it separately if required.",
            ));
            if ui.button(action).clicked() {
                surface.dispatch(crate::ui::ApplicationCommand::ApplyBootstrapSuggestion {
                    task_id: task.id.clone(),
                    generation: suggestion.turn_generation,
                });
            }
        });
    }
    let mut send = false;
    let mut cancel = false;
    if let Some(draft) = surface.bootstrap_conversation_draft(&task.id) {
        ui.add(
            egui::TextEdit::multiline(draft)
                .id_salt(("bootstrap-conversation-draft", &task.id))
                .desired_rows(3)
                .char_limit(1800)
                .hint_text("Ask about this setup step"),
        );
        ui.horizontal_wrapped(|ui| {
            if conversation.busy {
                if conversation.active_for_task && ui.button("Cancel response").clicked() {
                    cancel = true;
                } else if !conversation.active_for_task {
                    ui.label(crate::ui::theme::helper_text(
                        "Another setup-assistant response is in progress.",
                    ));
                }
            } else if ui
                .add_enabled(
                    !draft.trim().is_empty(),
                    egui::Button::new("Send to setup assistant").fill(crate::ui::theme::ACTION),
                )
                .clicked()
            {
                send = true;
            }
        });
    }
    if send {
        surface.dispatch(
            crate::ui::ApplicationCommand::SendBootstrapConversationMessage {
                task_id: task.id.clone(),
            },
        );
    }
    if cancel {
        surface.dispatch(crate::ui::ApplicationCommand::CancelBootstrapConversation {
            task_id: task.id.clone(),
        });
    }
}
