use super::*;
mod avatar;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    ticket: &str,
    view: &mut crate::ui::task_detail::ViewModel,
    brief: Option<&crate::core::attention::Brief>,
    height: f32,
) {
    let _ = brief;
    ui.label(RichText::new("Ask a question").font(theme::task_heading_font(ui.ctx(), 23.0)));
    ui.label(RichText::new("Get a quick answer from Kool.ad/e.").color(theme::TEXT_DIM));
    ui.add_space(20.0);
    let history_height = (height - 150.0).max(120.0);
    ui.allocate_ui(egui::vec2(ui.available_width(), history_height), |ui| {
        ui.set_min_height(history_height);
        ui.push_id(("task_modal_conversation", ticket), |ui| {
            paint_history(ui, &view.messages, view.conversation_active, history_height);
        });
    });
    ui.add_space(12.0);
    paint_reply(ui, s, ticket, view, &[]);
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
    if outcome.submit_decision.is_some() {
        s.dispatch(crate::ui::ApplicationCommand::TaskDetail(
            crate::ui::task_detail::Command::AskQuestion {
                ticket: ticket.to_owned(),
                draft: view.draft.clone(),
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

pub(super) fn paint_history(
    ui: &mut egui::Ui,
    messages: &[crate::domain::ChatMessage],
    _busy: bool,
    height: f32,
) {
    egui::ScrollArea::vertical()
        .max_height(height)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for message in messages {
                let user = message.role == crate::domain::ChatRole::User;
                let text = crate::ui::message_text::readable(message);
                let maximum = ((ui.available_width() - 52.0) * 0.9 - 32.0).max(40.0);
                let natural = ui
                    .painter()
                    .layout_no_wrap(
                        text.to_string(),
                        egui::FontId::proportional(18.0),
                        theme::TEXT,
                    )
                    .size()
                    .x;
                let width = natural.clamp(100.0_f32.min(maximum), maximum);
                ui.with_layout(
                    if user {
                        egui::Layout::right_to_left(egui::Align::TOP)
                    } else {
                        egui::Layout::left_to_right(egui::Align::TOP)
                    },
                    |ui| {
                        avatar::paint(ui, user);
                        egui::Frame::NONE
                            .fill(if user {
                                egui::Color32::from_rgb(0, 105, 245)
                            } else {
                                egui::Color32::from_rgb(26, 43, 56)
                            })
                            .corner_radius(14)
                            .inner_margin(16)
                            .show(ui, |ui| {
                                ui.set_width(width);
                                ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                                    ui.label(
                                        RichText::new(match message.role {
                                            crate::domain::ChatRole::User => "You",
                                            crate::domain::ChatRole::Agent => "Kool.ad/e",
                                            crate::domain::ChatRole::System => "Update",
                                        })
                                        .size(14.0)
                                        .color(theme::TEXT),
                                    );
                                    if user {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(text.as_ref()).size(18.0),
                                            )
                                            .wrap(),
                                        );
                                    } else {
                                        crate::ui::markdown::paint(
                                            ui,
                                            text.as_ref(),
                                            crate::ui::markdown::Style {
                                                body: 18.0,
                                                ..crate::ui::markdown::CHAT
                                            },
                                        );
                                    }
                                    if text.as_ref() != message.text {
                                        ui.push_id(&message.id, |ui| {
                                            ui.collapsing("Response details", |ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&message.text)
                                                            .monospace()
                                                            .small(),
                                                    )
                                                    .wrap(),
                                                );
                                            });
                                        });
                                    }
                                });
                            });
                    },
                );
                ui.add_space(16.0);
            }
        });
}
