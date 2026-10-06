//! Current task state and the evidence that explains it.
use super::*;

#[derive(Clone, Copy)]
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
    ui.add_space(6.0);
    let accent = status_color(&state);
    egui::Frame::NONE
        .fill(theme::PANEL_ALT)
        .corner_radius(10)
        .stroke(egui::Stroke::new(1.5, accent.gamma_multiply(0.65)))
        .inner_margin(if ui.available_width() < 500.0 { 10 } else { 14 })
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
                    .size(if ui.available_width() < 500.0 {
                        20.0
                    } else {
                        23.0
                    })
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
    } else if state.active {
        theme::BLUE_BRIGHT
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
                        crate::core::implementation::IndependentCheckStatus::Failed => {
                            theme::DANGER
                        }
                        crate::core::implementation::IndependentCheckStatus::Unavailable => {
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
        let message = crate::ui::message_text::readable(&state.view.messages[index]);
        let question = crate::ui::reply_tail::parse_reply_tail(&message)
            .ask
            .unwrap_or_else(|| crate::core::context_build::clip(&message, 140));
        ui.add(egui::Label::new(RichText::new(question).size(13.0)).wrap());
        ui.collapsing("Full question", |ui| {
            reply::full_message(ui, &message, "task_question")
        });
    } else if state.pull_request_closed {
        ui.label("The pull request closed before merging.");
    } else if state.interrupted {
        ui.label("The worker stopped. Preserved work is ready to resume.");
    } else if state.column == 2 {
        ui.label("Implementation is ready for review.");
    } else if state.column == 4 {
        ui.label("Implementation is complete.");
    } else {
        ui.label("Ready for Kool.ad/e to start this task.");
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
        ui.collapsing("Worker detail", |ui| {
            reply::full_message(ui, message, "worker_progress")
        });
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
                ui.label("Decision saved for Kool.ad/e.");
            }
        }
        Some(crate::core::attention::View::Loading) => {
            paint_saved_blocker(ui, error);
            ui.label(
                RichText::new("Simplifying this report… You can use the saved actions below now.")
                    .small()
                    .weak(),
            );
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        Some(crate::core::attention::View::Error(message)) => {
            paint_saved_blocker(ui, error);
            ui.colored_label(
                theme::WARNING,
                "The simpler explanation is unavailable. Showing the saved blocker.",
            );
            ui.collapsing("Explanation error", |ui| {
                ui.add(egui::Label::new(message).wrap());
            });
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
            paint_saved_blocker(ui, error);
        }
    }
    ui.collapsing("Full blocker report", |ui| {
        ui.set_width(ui.available_width());
        crate::ui::markdown::paint(ui, error, crate::ui::markdown::CHAT);
        if ui.small_button("Copy full report").clicked() {
            ui.ctx().copy_text(error.to_owned());
        }
    });
}

fn paint_saved_blocker(ui: &mut egui::Ui, error: &str) {
    let summary = error
        .trim()
        .strip_prefix("## Waiting for user action")
        .or_else(|| error.trim().strip_prefix("## Waiting for environment"))
        .unwrap_or(error)
        .trim();
    let summary = summary.split("\n### ").next().unwrap_or(summary);
    let summary = summary.split("\nFull report:").next().unwrap_or(summary);
    ui.add(egui::Label::new(crate::core::context_build::clip(summary, 1800)).wrap());
}
