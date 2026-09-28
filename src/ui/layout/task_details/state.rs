//! Current task state and the evidence that explains it.
use super::*;

pub(super) struct Presentation<'a> {
    pub ticket: &'a str,
    pub view: &'a crate::ui::task_detail::ViewModel,
    pub record: Option<&'a crate::core::implementation::Implementation>,
    pub failure: Option<&'a str>,
    pub cleanup_error: Option<&'a str>,
    pub status: &'a str,
    pub open_ask: Option<usize>,
    pub decision_sent: bool,
    pub column: usize,
    pub active: bool,
    pub pull_request_closed: bool,
    pub interrupted: bool,
}

pub(super) fn paint(ui: &mut egui::Ui, surface: &mut dyn Surface, state: Presentation<'_>) {
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
            ui.label(
                RichText::new(state.status)
                    .size(20.0)
                    .strong()
                    .color(status_color(&state)),
            );
            paint_independent_check(ui, state.record);
            paint_current_detail(ui, surface, &state);
        });
}

fn status_color(state: &Presentation<'_>) -> egui::Color32 {
    if state.failure.is_some()
        || state.cleanup_error.is_some()
        || state.interrupted
        || state.pull_request_closed
    {
        theme::WARNING
    } else if state.column == 4 {
        theme::SUCCESS
    } else {
        theme::TEXT
    }
}

fn paint_independent_check(
    ui: &mut egui::Ui,
    record: Option<&crate::core::implementation::Implementation>,
) {
    if let Some(check) = record.and_then(|record| record.independent_check.as_ref()) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("{} · {}", check.provider, check.status.label()))
                    .small()
                    .strong()
                    .color(match check.status {
                        crate::core::implementation::IndependentCheckStatus::Passed => {
                            theme::SUCCESS
                        }
                        crate::core::implementation::IndependentCheckStatus::Failed
                        | crate::core::implementation::IndependentCheckStatus::Unavailable => {
                            theme::WARNING
                        }
                        crate::core::implementation::IndependentCheckStatus::Pending => {
                            theme::TEXT_DIM
                        }
                    }),
            );
            if let Some(commit) = check.commit.get(..12) {
                ui.label(RichText::new(format!("commit {commit}")).small().weak());
            }
        });
        if let Some(detail) = &check.detail {
            ui.add(egui::Label::new(detail).wrap());
        }
    }
}

fn paint_current_detail(ui: &mut egui::Ui, surface: &mut dyn Surface, state: &Presentation<'_>) {
    if state.active {
        paint_progress(ui, state.view);
    } else if let Some(error) = state.failure {
        paint_failure(ui, surface, state, error);
    } else if let Some(error) = state.cleanup_error {
        reply::full_message(ui, error, "cleanup_failure");
    } else if let Some(index) = state.open_ask {
        reply::full_message(
            ui,
            crate::ui::message_text::readable(&state.view.messages[index]).as_ref(),
            "task_question",
        );
    } else if state.pull_request_closed {
        ui.label("The pull request closed before merging.");
    } else if state.interrupted {
        ui.label("The worker stopped. Preserved work is ready to resume.");
    } else if state.column == 2 {
        ui.label("Implementation is ready for review.");
    } else if state.column == 4 {
        ui.label("Implementation is complete.");
    } else {
        ui.label("Ready for Packet to start this task.");
    }
}

fn paint_progress(ui: &mut egui::Ui, view: &crate::ui::task_detail::ViewModel) {
    if let Some(progress) = view.progress.as_ref() {
        let message = progress
            .activity
            .as_deref()
            .filter(|text| !text.is_empty())
            .or_else(|| (!progress.response.is_empty()).then_some(progress.response.as_str()))
            .or_else(|| (!progress.thoughts.is_empty()).then_some(progress.thoughts.as_str()))
            .unwrap_or("Worker is starting.");
        reply::full_message(ui, message, "worker_progress");
    } else {
        ui.label("Worker is starting.");
    }
}

fn paint_failure(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    state: &Presentation<'_>,
    error: &str,
) {
    match &state.view.attention {
        Some(crate::core::attention::View::Ready(brief)) => {
            ui.add(egui::Label::new(&brief.problem).wrap());
            if state.decision_sent {
                ui.label("Decision saved for Packet.");
            }
        }
        Some(crate::core::attention::View::Loading) => {
            ui.label("Preparing a plain-language explanation of this blocker…");
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        Some(crate::core::attention::View::Error(message)) => {
            ui.colored_label(theme::WARNING, "Could not prepare the explanation.");
            ui.add(egui::Label::new(message).wrap());
            if ui.small_button("Retry explanation").clicked() {
                surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
                    crate::ui::task_detail::Command::RetryExplanation {
                        ticket: state.ticket.to_owned(),
                        detail: error.to_owned(),
                    },
                ));
            }
        }
        None => {
            ui.label(failure_summary(error));
        }
    }
    ui.label(egui::RichText::new("Full blocker report").strong());
    ui.set_width(ui.available_width());
    crate::ui::markdown::paint(ui, error, crate::ui::markdown::CHAT);
    if ui.small_button("Copy full report").clicked() {
        ui.ctx().copy_text(error.to_owned());
    }
}
