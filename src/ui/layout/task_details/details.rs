use super::*;
mod metrics;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
    presentation: state::Presentation<'_>,
    checks_unavailable: bool,
    failure: Option<&str>,
    activity_path: &mut Option<String>,
) {
    ui.heading("Task details & state");
    state::paint(ui, s, presentation);
    let view = presentation.view;
    let record = presentation.record;
    let ticket = presentation.ticket;
    let active = presentation.active;
    let column = presentation.column;
    let brief = match &view.attention {
        Some(crate::core::attention::View::Ready(brief)) => Some(brief),
        _ => None,
    };
    let decision_sent = presentation.decision_sent;
    ui.add_space(10.0);
    egui::Frame::NONE
        .fill(egui::Color32::from_rgb(14, 31, 44))
        .stroke(egui::Stroke::new(1.5, theme::BLUE))
        .corner_radius(10)
        .inner_margin(if ui.available_width() < 500.0 { 10 } else { 14 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("YOUR NEXT STEP")
                    .size(10.5)
                    .strong()
                    .color(theme::BLUE_BRIGHT),
            );
            if active {
                ui.label("Kool.ad/e is working. You can cancel this task while unrelated work continues.");
                if ui
                    .add(
                        egui::Button::new("Cancel task")
                            .min_size(egui::vec2(0.0, 34.0))
                            .stroke(egui::Stroke::new(1.0, theme::DANGER)),
                    )
                    .clicked()
                {
                    dispatch(
                        s,
                        crate::ui::task_detail::Command::CancelTask {
                            ticket: ticket.to_owned(),
                        },
                    );
                }
            } else if let Some(url) = record.and_then(|record| record.pr_url.as_ref()) {
                ui.label(if presentation.pull_request_closed {
                    "Reopen the pull request on GitHub to continue review."
                } else {
                    "Review the published changes."
                });
                ui.hyperlink_to("Open PR", url);
            } else if column == 4 {
                ui.label("No action needed.");
            } else if review::changes_requested(record) {
                review::paint_changes_requested(ui);
            } else if review::approval_required(record) {
                review::paint_approval(ui, s, ticket);
            } else {
                paint_attention(ui, brief, decision_sent, failure, checks_unavailable);
                let label = if record.is_some_and(|record| {
                    record.status
                        == crate::core::implementation::ImplementationStatus::ReadyToPublish
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
                        egui::Button::new(RichText::new(label).strong().color(theme::TEXT))
                            .fill(theme::BLUE)
                            .min_size(egui::vec2(0.0, 36.0)),
                    )
                    .clicked()
                {
                    dispatch(
                        s,
                        crate::ui::task_detail::Command::StartOrResume {
                            ticket: ticket.to_owned(),
                        },
                    );
                }
            }
        });
    activity::paint(ui, view, ticket, active, activity_path);
    metrics::paint(ui, view);
    let checklist = crate::ui::task_checklist::from_task(&doc.text, s.implementation_state(ticket));
    if !checklist.is_empty() {
        ui.collapsing("Checklist", |ui| {
            let elapsed = s.implementation_elapsed(ticket);
            crate::ui::task_checklist::paint(
                ui,
                &checklist,
                column == 4,
                false,
                active,
                elapsed.as_deref(),
            );
        });
    }
    ui.collapsing("Task description & acceptance criteria", |ui| {
        crate::ui::spec_viewer::render(ui, Some(&doc.text));
    });
    ui.collapsing("Technical details", |ui| paint_task_properties(ui, s, doc));
}

fn paint_attention(
    ui: &mut egui::Ui,
    brief: Option<&crate::core::attention::Brief>,
    decision_sent: bool,
    failure: Option<&str>,
    checks_unavailable: bool,
) {
    if let Some(brief) = brief {
        if decision_sent {
            ui.label("Decision saved in this task's conversation.");
        }
        if !brief.options.is_empty() {
            ui.label("Kool.ad/e is waiting for your decision. Choose an option in the conversation on the left.");
        } else if !brief.steps.is_empty() {
            ui.label("Kool.ad/e is waiting for the action listed below. Your work is safe.");
        } else {
            ui.label("Kool.ad/e couldn't complete this check. Your work is safe. Resume implementation to try again.");
        }
        reply::paint_recommendation(ui, brief);
        for step in &brief.steps {
            ui.add(egui::Label::new(format!("{}: {}", step.owner, step.action)).wrap());
        }
        ui.label(RichText::new(&brief.after).small().weak());
    } else if checks_unavailable {
        ui.label("Kool.ad/e couldn't complete the project check. Your work is safe and hasn't been published.");
    } else if let Some(error) = failure {
        let actions = reply::failure_actions(error);
        if actions.is_empty() {
            ui.label(
                "Review the blocker and reply in the conversation. Resume after it is resolved.",
            );
        } else {
            for action in actions {
                ui.add(egui::Label::new(format!("• {action}")).wrap());
            }
        }
    } else {
        ui.label("Start implementation when this task is ready.");
    }
}

fn dispatch(s: &mut dyn Surface, command: crate::ui::task_detail::Command) {
    s.dispatch(crate::ui::ApplicationCommand::TaskDetail(command));
}
