use crate::{domain::ChatRole, ui::theme};

pub(super) fn paint(ui: &mut egui::Ui, surface: &mut dyn crate::ui::Surface) {
    ui.heading("Project activity");
    ui.label("Manager supervision and earlier unscoped history appear here, separate from Specification chat.");
    ui.add_space(12.0);

    theme::card_frame().show(ui, |ui| {
        ui.label(theme::section_heading("Man.ager supervision"));
        if surface.manager_running() {
            let route = surface
                .manager_progress()
                .and_then(|progress| progress.selected_route.as_deref())
                .unwrap_or("Configured provider");
            ui.label("Manager is checking the current project board.");
            ui.label(theme::metadata_text(format!("Route: {route}")));
            if let Some(progress) = surface.manager_progress() {
                ui.label(theme::section_heading("Live provider progress"));
                crate::ui::chat_pane::paint_progress(ui, progress);
            }
        } else if surface.manager_activity().is_empty() {
            ui.label("Manager has not recorded a board check yet.");
        } else {
            ui.label("Latest board check:");
        }
        if let Some(error) = surface.manager_history_error() {
            ui.colored_label(theme::WARNING, error);
        }
        if let Some(error) = surface.manager_last_error() {
            ui.colored_label(
                theme::WARNING,
                format!("Manager check needs attention: {error}"),
            );
            if ui.button("Retry Manager check").clicked() {
                surface.retry_manager_check();
            }
        }
        egui::ScrollArea::vertical()
            .id_salt("manager_activity_history")
            .max_height(380.0)
            .show(ui, |ui| {
                for message in surface.manager_activity().iter().rev() {
                    paint_history_message(ui, message, Some("Man.ager"));
                }
            });
    });

    let legacy = surface.legacy_chat_messages();
    ui.add_space(12.0);
    ui.collapsing(
        format!("Earlier unscoped history · {} entries", legacy.len()),
        |ui| {
            ui.label("This history is preserved for audit and is excluded from future Specification prompts.");
            egui::ScrollArea::vertical()
                .id_salt("legacy_chat_archive")
                .max_height(480.0)
                .show(ui, |ui| {
                    for message in legacy.iter().rev() {
                        paint_history_message(ui, message, None);
                    }
                });
        },
    );
}

fn paint_history_message(
    ui: &mut egui::Ui,
    message: &crate::domain::ChatMessage,
    author_override: Option<&str>,
) {
    ui.push_id(&message.id, |ui| {
        ui.horizontal(|ui| {
            let author = author_override.unwrap_or(match message.role {
                ChatRole::User => "You",
                ChatRole::Agent => "Unscoped assistant",
                ChatRole::System => "Status",
            });
            ui.label(theme::metadata_text(format!(
                "{author} · {}",
                message.ts.format("%Y-%m-%d %H:%M")
            )));
        });
        if message.role == ChatRole::Agent {
            let readable = crate::ui::message_text::readable(message);
            crate::ui::markdown::paint(ui, &readable, crate::ui::markdown::CHAT);
        } else {
            ui.label(message.text.trim());
        }
        ui.add_space(8.0);
    });
}
