//! Inline task replies and complete state messages.
use super::*;
use crate::domain::ChatMessage;

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

pub(super) fn paint_recommendation(ui: &mut egui::Ui, brief: &crate::core::attention::Brief) {
    let Some(recommendation) = &brief.recommendation else {
        return;
    };
    let Some(option) = brief
        .options
        .iter()
        .find(|option| option.id == recommendation.option_id)
    else {
        return;
    };
    ui.label(
        RichText::new("Kool.ad/e recommends")
            .strong()
            .color(theme::ACCENT),
    );
    ui.add(egui::Label::new(format!("{}: {}", option.label, recommendation.rationale)).wrap());
    ui.add(egui::Label::new(format!("If chosen: {}", option.consequence)).wrap());
}

pub(super) struct Outcome {
    pub submit_decision: Option<bool>,
    pub retry_save: bool,
}

pub(super) fn paint(
    ui: &mut egui::Ui,
    key: &str,
    draft: &mut String,
    busy: bool,
    _messages: &[ChatMessage],
    _blocker_choices: &[crate::core::attention::OptionBrief],
    chat_error: Option<&str>,
) -> Outcome {
    let mut submit_decision = None;
    let mut retry_save = false;
    ui.push_id(("task_detail_reply", key), |ui| {
        egui::Frame::NONE
            .fill(theme::PANEL_ALT)
            .corner_radius(14)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 98.0).max(40.0);
                    ui.visuals_mut().extreme_bg_color = egui::Color32::from_rgb(26, 43, 56);
                    let response = egui::TextEdit::multiline(draft)
                        .desired_rows(2)
                        .desired_width(width)
                        .font(egui::FontId::proportional(16.0))
                        .hint_text("Ask for clarification…")
                        .id_salt("task_detail_reply_text")
                        .show(ui);
                    let enabled = !busy && !draft.trim().is_empty();
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Button::new(RichText::new("Send").size(17.0))
                                .fill(egui::Color32::from_rgb(0, 105, 245))
                                .min_size(egui::vec2(84.0, 50.0)),
                        )
                        .clicked()
                        || (enabled
                            && response.response.has_focus()
                            && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)))
                    {
                        submit_decision = Some(false);
                    }
                });
            });
        if busy {
            ui.label(RichText::new("Kool.ad/e is responding…").small().weak());
        }
        if let Some(error) = chat_error {
            ui.colored_label(theme::WARNING, error);
            retry_save = ui.small_button("Retry saving response").clicked();
        }
    });
    Outcome {
        submit_decision,
        retry_save,
    }
}
