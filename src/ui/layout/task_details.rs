//! Conversation and task state for the task modal.
use super::*;
mod action;
mod activity;
mod conversation;
mod dependency;
mod details;
mod lifecycle;
mod planning;
mod reply;
mod review;
mod state;

pub(super) fn machine_setup_summary(
    task: &crate::persistence::setup_tasks::LocalTask,
) -> &'static str {
    planning::machine_setup_summary(task)
}

pub(super) fn machine_setup_title(task: &crate::persistence::setup_tasks::LocalTask) -> String {
    planning::machine_setup_title(task)
}

#[cfg(test)]
pub(super) fn seed_custom_target_repository_root_draft(
    context: &egui::Context,
    ticket: &str,
    app_surface: crate::project_runtime::AppSurface,
) {
    details::runtime_capability::seed_custom_target_repository_root_draft(
        context,
        ticket,
        app_surface,
    );
}

pub(super) fn paint_machine_setup(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    task: &crate::persistence::setup_tasks::LocalTask,
) {
    planning::paint_machine_setup(ui, surface, board, task);
}

pub(super) fn paint_clarification(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    question: &crate::core::planning::QuestionV1,
    height: f32,
) {
    planning::paint_clarification(ui, surface, question, height);
}

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
    height: f32,
    activity_path: &mut Option<String>,
) {
    if ui.available_width() < 500.0 {
        ui.spacing_mut().item_spacing.y = 4.0;
    }
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
    let cleanup_error = record
        .as_ref()
        .and_then(|record| record.cleanup.error.clone());
    let failure = view.failure.clone();
    let brief = match &view.attention {
        Some(crate::core::attention::View::Ready(brief)) => Some(brief.clone()),
        _ => None,
    };
    let open_ask = crate::ui::reply_tail::open_ask_index(&view.messages);
    let legacy_current = view
        .messages
        .iter()
        .rev()
        .find(|message| {
            message.role == crate::domain::ChatRole::Agent
                && message.id.starts_with("needs-attention:")
        })
        .is_some_and(|message| view.answer_context.as_deref() == Some(message.id.as_str()));
    let decision_sent = brief.as_ref().is_some_and(|brief| {
        view.messages
            .iter()
            .rev()
            .take_while(|message| {
                message.role != crate::domain::ChatRole::Agent
                    || !(message.id.starts_with("needs-attention:")
                        || message.text.starts_with("## What Happened"))
            })
            .any(|message| {
                message.role == crate::domain::ChatRole::User
                    && !message.clarification
                    && ((message.answer_context.is_some()
                        && message.answer_context == view.answer_context
                        && !message.text.trim().is_empty())
                        || (message.answer_context.is_none()
                            && legacy_current
                            && brief.options.iter().any(|option| {
                                message
                                    .text
                                    .starts_with(&format!("I choose option ({}):", option.id))
                            })))
            })
    });
    let column = view.board_column;
    let pull_request_closed = record.as_ref().is_some_and(|record| {
        record.pr_state == Some(crate::core::implementation::PullRequestState::Closed)
            || record.status == crate::core::implementation::ImplementationStatus::PullRequestClosed
    });
    let interrupted = !active
        && record.as_ref().is_some_and(|record| {
            matches!(
                record.status,
                crate::core::implementation::ImplementationStatus::Preparing
                    | crate::core::implementation::ImplementationStatus::Implementing
                    | crate::core::implementation::ImplementationStatus::Verifying
                    | crate::core::implementation::ImplementationStatus::Publishing
                    | crate::core::implementation::ImplementationStatus::WaitingToMerge
                    | crate::core::implementation::ImplementationStatus::Interrupted
            )
        });
    let status = if active {
        record
            .as_ref()
            .map(|record| record.status.label())
            .unwrap_or("Starting")
    } else if let Some(task_status) = view.task_status {
        if task_status == crate::core::planning_work::WorkStatus::Done && cleanup_error.is_some() {
            "Done · cleanup needs attention"
        } else {
            crate::core::implementation::BOARD_COLUMNS[task_status.board_column()]
        }
    } else if column == 4 && cleanup_error.is_some() {
        "Done · cleanup needs attention"
    } else if pull_request_closed {
        "PR closed"
    } else if failure.is_some() {
        "Needs attention"
    } else if interrupted {
        "Interrupted"
    } else {
        record
            .as_ref()
            .map(|record| record.status.label())
            .unwrap_or(crate::core::implementation::BOARD_COLUMNS[column])
    };
    let mut conversation_view = view.clone();
    let presentation = state::Presentation {
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
    };
    if ui.available_width() >= 760.0 {
        let available = ui.available_size();
        let gap = 28.0;
        let left = (available.x - gap) * 0.6;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(left, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("task_action", ticket))
                        .max_height(height)
                        .show(ui, |ui| {
                            action::paint(
                                ui,
                                s,
                                doc,
                                presentation,
                                brief.as_ref(),
                                checks_unavailable,
                                activity_path,
                            );
                        });
                },
            );
            ui.separator();
            ui.add_space(12.0);
            ui.allocate_ui_with_layout(
                egui::vec2((available.x - left - gap).max(0.0), height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    conversation::paint(ui, s, ticket, &mut conversation_view, None, height);
                },
            );
        });
    } else {
        action::paint(
            ui,
            s,
            doc,
            presentation,
            brief.as_ref(),
            checks_unavailable,
            activity_path,
        );
        ui.add_space(20.0);
        ui.separator();
        conversation::paint(ui, s, ticket, &mut conversation_view, None, height);
    }
}

pub(super) fn paint_planning_item(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    item: &crate::domain::item::OpenItem,
    height: f32,
) {
    planning::paint_item(ui, surface, board, item, height);
}

pub(super) fn paint_planning_work(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    work: &crate::core::planning_work::Work,
    height: f32,
) {
    planning::paint_work(ui, surface, board, work, height);
}

pub(super) fn paint_opportunity(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    proposal: &crate::core::opportunity::OpportunityV1,
    record_revision: u64,
    height: f32,
) {
    planning::paint_opportunity(ui, surface, board, proposal, record_revision, height);
}

pub(super) fn paint_setup_issue(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    issue: &crate::app::setup_attention::SetupIssue,
) {
    planning::paint_setup_issue(ui, surface, board, issue);
}

pub(super) fn paint_feature_approval(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    action: &crate::ui::feature_approval::Action,
) {
    planning::paint_feature_approval(ui, surface, action);
}

pub(super) fn paint_plan_review(
    ui: &mut egui::Ui,
    review: &crate::ui::task_detail::LifecyclePlanReviewView,
) {
    planning::paint_plan_review(ui, review);
}
