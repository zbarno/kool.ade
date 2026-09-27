//! State, reply, and activity for the task modal.
use super::*;
mod activity;
mod reply;
mod state;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
    _height: f32,
    activity_path: &mut Option<String>,
) {
    ui.label(
        RichText::new(task_key(&doc.path))
            .small()
            .color(theme::TEXT_DIM),
    );
    ui.heading(&doc.title);
    if doc.path.ends_with("/README.md") {
        crate::ui::spec_viewer::render(ui, Some(&doc.text));
        return;
    }
    let ticket = &doc.path;
    let Some(mut view) = s.task_detail_view(ticket) else {
        ui.label("Task state is unavailable.");
        return;
    };
    let record = view.implementation.take();
    let checks_unavailable = record.as_ref().is_some_and(|record| {
        record.independent_check.as_ref().is_some_and(|check| {
            check.status == crate::core::implementation::IndependentCheckStatus::Unavailable
        })
    });
    let active = view.implementation_active;
    let cleanup_error = record.as_ref().and_then(|r| r.cleanup.error.clone());
    let failure = view.failure.clone();
    let brief = match &view.attention {
        Some(crate::core::attention::View::Ready(brief)) => Some(brief.clone()),
        _ => None,
    };
    let open_ask = crate::ui::reply_tail::open_ask_index(&view.messages);
    let decision_sent = brief
        .as_ref()
        .is_some_and(|brief| !brief.options.is_empty())
        && view
            .messages
            .iter()
            .rev()
            .find(|m| m.role == crate::domain::ChatRole::User)
            .is_some_and(|m| m.text.starts_with("I choose option ("));
    let column = view.board_column;
    let pull_request_closed = record.as_ref().is_some_and(|r| {
        r.pr_state == Some(crate::core::implementation::PullRequestState::Closed)
            || r.status == crate::core::implementation::ImplementationStatus::PullRequestClosed
    });
    let interrupted = !active
        && record.as_ref().is_some_and(|r| {
            matches!(
                r.status,
                crate::core::implementation::ImplementationStatus::Preparing
                    | crate::core::implementation::ImplementationStatus::Implementing
                    | crate::core::implementation::ImplementationStatus::Verifying
                    | crate::core::implementation::ImplementationStatus::Publishing
                    | crate::core::implementation::ImplementationStatus::WaitingToMerge
                    | crate::core::implementation::ImplementationStatus::ReadyToPublish
                    | crate::core::implementation::ImplementationStatus::Interrupted
            )
        });
    let status = if column == 4 && cleanup_error.is_some() {
        "Done · cleanup needs attention"
    } else if pull_request_closed {
        "PR closed"
    } else if active {
        record
            .as_ref()
            .map(|r| r.status.label())
            .unwrap_or("Starting")
    } else if failure.is_some() {
        "Needs attention"
    } else if interrupted {
        "Interrupted"
    } else {
        record
            .as_ref()
            .map(|r| r.status.label())
            .unwrap_or(crate::core::implementation::BOARD_COLUMNS[column])
    };
    state::paint(
        ui,
        s,
        state::Presentation {
            ticket,
            view: &view,
            record: record.as_ref(),
            failure: failure.as_deref(),
            cleanup_error: cleanup_error.as_deref(),
            status,
            open_ask,
            decision_sent,
            column,
            active,
            pull_request_closed,
            interrupted,
        },
    );
    ui.add_space(10.0);
    egui::Frame::NONE
        .fill(theme::ACCENT_SOFT)
        .corner_radius(8)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("YOUR NEXT STEP")
                    .size(10.5)
                    .strong()
                    .color(theme::ACCENT),
            );
            if active {
                ui.label("Packet is working. You can stop this task and pause the queue.");
                if ui.button("Stop task and pause queue").clicked() {
                    s.dispatch(crate::ui::ApplicationCommand::TaskDetail(crate::ui::task_detail::Command::StopAndPause {
                        ticket: ticket.to_owned(),
                    }));
                }
            } else if let Some(url) = record.as_ref().and_then(|r| r.pr_url.as_ref()) {
                ui.label(if pull_request_closed {
                    "Reopen the pull request on GitHub to continue review."
                } else {
                    "Review the published changes."
                });
                ui.hyperlink_to("Open PR", url);
            } else if column == 4 {
                ui.label("No action needed.");
            } else {
                if let Some(brief) = &brief {
                    if decision_sent {
                        ui.label("Decision saved in this task's conversation.");
                    }
                    reply::paint_recommendation(ui, brief);
                    for step in &brief.steps {
                        ui.add(egui::Label::new(format!("{}: {}", step.owner, step.action)).wrap());
                    }
                    if !brief.options.is_empty() {
                        if decision_sent {
                            ui.collapsing("Change decision", |ui| {
                                paint_reply(ui, s, ticket, &mut view, &brief.options);
                            });
                        } else {
                            paint_reply(ui, s, ticket, &mut view, &brief.options);
                        }
                    }
                    ui.label(RichText::new(&brief.after).small().weak());
                } else if checks_unavailable {
                    ui.label("Resolve the project check problem shown above, then resume this task. Packet has kept the verified work and has not published it to the default branch.");
                } else if matches!(&view.attention, Some(crate::core::attention::View::Loading | crate::core::attention::View::Error(_))) {
                    ui.label("The full report contains the original actions while Packet prepares a clearer explanation.");
                } else if let Some(error) = &failure {
                    let actions = reply::failure_actions(error);
                    if actions.is_empty() {
                        ui.label("Review the failure, then resume the preserved work.");
                    } else {
                        for action in actions {
                            ui.add(egui::Label::new(format!("• {action}")).wrap());
                        }
                    }
                } else if record.as_ref().is_some_and(|record| {
                    record.status == crate::core::implementation::ImplementationStatus::ReadyToPublish
                }) {
                    ui.label("Verification is complete. The work is saved locally; Auto Publish is off. Choose Share verified work for review when ready.");
                } else if open_ask.is_some() {
                    ui.label("Answer Packet's question below.");
                } else if interrupted {
                    ui.label("Resume the preserved implementation.");
                } else {
                    ui.label("Start implementation when this task is ready.");
                }
                let label = if record.as_ref().is_some_and(|record| {
                    record.status == crate::core::implementation::ImplementationStatus::ReadyToPublish
                }) {
                    "Share verified work for review"
                } else if checks_unavailable
                    || view.failure_disposition
                        == Some(crate::core::implementation::RecoveryDisposition::UserAction)
                {
                    "Resume after action"
                } else if record.is_some() || failure.is_some() {
                    "Resume implementation"
                } else if view.auto_build {
                    "Implement & continue queue"
                } else {
                    "Implement"
                };
                if ui
                    .add_enabled(
                        view.can_start,
                        egui::Button::new(label).fill(theme::PANEL_ALT),
                    )
                    .clicked()
                {
                    s.dispatch(crate::ui::ApplicationCommand::TaskDetail(crate::ui::task_detail::Command::StartOrResume {
                        ticket: ticket.to_owned(),
                    }));
                }
            }
        });
    activity::paint(ui, &view, ticket, active, activity_path);
    if !active
        && column != 4
        && (open_ask.is_some() || failure.is_some())
        && brief.as_ref().is_none_or(|brief| brief.options.is_empty())
    {
        ui.add_space(10.0);
        egui::Frame::NONE
            .fill(theme::ACCENT_SOFT)
            .corner_radius(8)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                paint_reply(ui, s, ticket, &mut view, &[]);
            });
    }
    ui.add_space(12.0);
    ui.separator();
    ui.collapsing("Discussion history", |ui| {
        crate::ui::task_chat::paint_history_messages(ui, &view.messages, view.conversation_active)
    });
    ui.collapsing("Task description & acceptance criteria", |ui| {
        crate::ui::spec_viewer::render(ui, Some(&doc.text));
    });
    ui.collapsing("Technical details", |ui| paint_task_properties(ui, s, doc));
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
