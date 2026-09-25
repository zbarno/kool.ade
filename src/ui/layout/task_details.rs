//! State, reply, and activity for the task modal.
use super::*;
mod blocker;
mod reply;

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
    let record = s.implementation_state(ticket).cloned();
    let active = s.implementation_active(ticket);
    let cleanup_error = record.as_ref().and_then(|r| r.cleanup.error.clone());
    let failure = s
        .implementation_failure(ticket)
        .map(str::to_owned)
        .or_else(|| {
            record
                .as_ref()
                .filter(|r| r.status == "Needs attention")
                .map(|r| r.detail.clone())
        });
    let blocker = failure.as_deref().and_then(blocker::parse);
    let messages = s.task_messages(ticket).to_vec();
    let open_ask = crate::ui::reply_tail::open_ask_index(&messages);
    let decision_sent = blocker
        .as_ref()
        .is_some_and(|plan| !plan.choices.is_empty())
        && messages
            .iter()
            .rev()
            .find(|m| m.role == crate::domain::ChatRole::User)
            .is_some_and(|m| m.text.starts_with("I choose option ("));
    let column = task_board_column(s, ticket);
    let status = if column == 4 && cleanup_error.is_some() {
        "Done · cleanup needs attention"
    } else if record
        .as_ref()
        .is_some_and(|r| r.pr_state.as_deref() == Some("CLOSED"))
    {
        "PR closed"
    } else if active {
        record
            .as_ref()
            .map(|r| r.status.as_str())
            .unwrap_or("Starting")
    } else if failure.is_some() {
        "Needs attention"
    } else if record.as_ref().is_some_and(|r| {
        matches!(
            r.status.as_str(),
            "Preparing"
                | "Implementing"
                | "Verifying"
                | "Publishing"
                | "Waiting to merge"
                | "Ready for PR"
                | "Interrupted"
        )
    }) {
        "Interrupted"
    } else {
        record
            .as_ref()
            .map(|r| r.status.as_str())
            .unwrap_or(crate::core::implementation::BOARD_COLUMNS[column])
    };
    ui.add_space(10.0);
    egui::Frame::NONE
        .fill(theme::PANEL_ALT)
        .corner_radius(8)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("CURRENT STATE")
                    .size(10.5)
                    .strong()
                    .color(theme::TEXT_DIM),
            );
            ui.label(RichText::new(status).size(20.0).strong().color(
                if failure.is_some()
                    || cleanup_error.is_some()
                    || matches!(status, "Interrupted" | "PR closed")
                {
                    theme::WARNING
                } else if column == 4 {
                    theme::SUCCESS
                } else {
                    theme::TEXT
                },
            ));
            if active {
                if let Some(progress) = s.task_progress(ticket) {
                    let message = progress
                        .activity
                        .as_deref()
                        .filter(|text| !text.is_empty())
                        .or_else(|| {
                            (!progress.response.is_empty()).then_some(progress.response.as_str())
                        })
                        .or_else(|| {
                            (!progress.thoughts.is_empty()).then_some(progress.thoughts.as_str())
                        })
                        .unwrap_or("Worker is starting.");
                    reply::full_message(ui, message, "worker_progress");
                } else {
                    ui.label("Worker is starting.");
                }
            } else if let Some(error) = &failure {
                if let Some(plan) = &blocker {
                    ui.label(if decision_sent {
                        "Decision saved for Packet. Complete any remaining external checks, then resume."
                    } else { &plan.summary });
                } else {
                    ui.label(failure_summary(error));
                }
                ui.collapsing("Full report", |ui| {
                    reply::full_message(ui, error, "implementation_failure");
                });
            } else if let Some(error) = &cleanup_error {
                reply::full_message(ui, error, "cleanup_failure");
            } else if let Some(index) = open_ask {
                reply::full_message(
                    ui,
                    crate::ui::message_text::readable(&messages[index]).as_ref(),
                    "task_question",
                );
            } else if status == "PR closed" {
                ui.label("The pull request closed before merging.");
            } else if status == "Interrupted" {
                ui.label("The worker stopped. Preserved work is ready to resume.");
            } else if column == 2 {
                ui.label("Implementation is ready for review.");
            } else if column == 4 {
                ui.label("Implementation is complete.");
            } else {
                ui.label("Ready for Packet to start this task.");
            }
        });
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
                    s.cancel_task_for(ticket);
                }
            } else if let Some(url) = record.as_ref().and_then(|r| r.pr_url.as_ref()) {
                ui.label(if status == "PR closed" {
                    "Reopen the pull request on GitHub to continue review."
                } else {
                    "Review the published changes."
                });
                ui.hyperlink_to("Open PR", url);
            } else if column == 4 {
                ui.label("No action needed.");
            } else {
                if let Some(plan) = &blocker {
                    if decision_sent {
                        ui.label("Decision saved in this task's conversation.");
                    }
                    for step in plan
                        .steps
                        .iter()
                        .filter(|step| !decision_sent || !step.starts_with("Adjudicator:"))
                    {
                        ui.add(egui::Label::new(step).wrap());
                    }
                    if !plan.choices.is_empty() {
                        if decision_sent {
                            ui.collapsing("Change decision", |ui| {
                                reply::paint(ui, s, ticket, &messages, &plan.choices);
                            });
                        } else {
                            reply::paint(ui, s, ticket, &messages, &plan.choices);
                        }
                    }
                } else if let Some(error) = &failure {
                    let actions = reply::failure_actions(error);
                    if actions.is_empty() {
                        ui.label("Review the failure, then resume the preserved work.");
                    } else {
                        for action in actions {
                            ui.add(egui::Label::new(format!("• {action}")).wrap());
                        }
                    }
                } else if open_ask.is_some() {
                    ui.label("Answer Packet's question below.");
                } else if status == "Interrupted" {
                    ui.label("Resume the preserved implementation.");
                } else {
                    ui.label("Start implementation when this task is ready.");
                }
                let label = if failure
                    .as_ref()
                    .is_some_and(|error| error.contains("## Waiting for user action"))
                {
                    "Resume after action"
                } else if record.is_some() {
                    "Resume implementation"
                } else if s.auto_mode() {
                    "Implement & continue queue"
                } else {
                    "Implement"
                };
                if ui
                    .add_enabled(
                        s.implementation_capacity(),
                        egui::Button::new(label).fill(theme::PANEL_ALT),
                    )
                    .clicked()
                {
                    s.implement_task(ticket.to_string());
                }
            }
        });
    ui.add_space(14.0);
    ui.label(RichText::new("Activity").strong().size(16.0));
    let samples = s.activity_samples(Some(ticket));
    crate::ui::task_activity::graph(ui, &samples, s.activity_active(ticket), 76.0);
    if let Some(progress) = s.task_progress(ticket) {
        ui.label(RichText::new(crate::ui::task_activity::preview(progress)).small());
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(crate::ui::task_activity::timing(progress, active))
                    .small()
                    .weak(),
            );
            if ui.small_button("View all activity").clicked() {
                *activity_path = Some(ticket.clone());
            }
        });
    } else {
        ui.label(
            RichText::new("No worker activity recorded yet.")
                .small()
                .weak(),
        );
    }
    if !active
        && column != 4
        && (open_ask.is_some() || failure.is_some())
        && blocker.as_ref().is_none_or(|plan| plan.choices.is_empty())
    {
        ui.add_space(10.0);
        egui::Frame::NONE
            .fill(theme::ACCENT_SOFT)
            .corner_radius(8)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                reply::paint(ui, s, ticket, &messages, &[]);
            });
    }
    ui.add_space(12.0);
    ui.separator();
    ui.collapsing("Discussion history", |ui| {
        crate::ui::task_chat::paint_history(ui, s, ticket)
    });
    ui.collapsing("Task description & acceptance criteria", |ui| {
        crate::ui::spec_viewer::render(ui, Some(&doc.text));
    });
    ui.collapsing("Technical details", |ui| paint_task_properties(ui, s, doc));
}
