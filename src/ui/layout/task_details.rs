//! Conversation and task state for the task modal.
use super::*;
mod activity;
mod conversation;
mod details;
mod hero;
mod planning;
mod reply;
mod review;
mod state;

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
    hero::paint(ui, doc);
    if doc.path.ends_with("/README.md") {
        crate::ui::spec_viewer::render(ui, Some(&doc.text));
        return;
    }
    let ticket = &doc.path;
    s.dispatch(crate::ui::ApplicationCommand::PrepareTaskChat {
        key: ticket.to_owned(),
    });
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
    let decision_sent = brief
        .as_ref()
        .is_some_and(|brief| !brief.options.is_empty())
        && view
            .messages
            .iter()
            .rev()
            .find(|message| message.role == crate::domain::ChatRole::User)
            .is_some_and(|message| message.text.starts_with("I choose option ("));
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
    let status = if column == 4 && cleanup_error.is_some() {
        "Done · cleanup needs attention"
    } else if pull_request_closed {
        "PR closed"
    } else if active {
        record
            .as_ref()
            .map(|record| record.status.label())
            .unwrap_or("Starting")
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
    if ui.available_width() >= 1000.0 {
        ui.columns(2, |columns| {
            egui::ScrollArea::vertical()
                .id_salt(("task_conversation_pane", ticket))
                .max_height(height)
                .auto_shrink([false, false])
                .show(&mut columns[0], |ui| {
                    conversation::paint(ui, s, ticket, &mut view, brief.as_ref(), height);
                });
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
            egui::ScrollArea::vertical()
                .id_salt(("task_details_pane", ticket))
                .max_height(height)
                .auto_shrink([false, false])
                .show(&mut columns[1], |ui| {
                    details::paint(
                        ui,
                        s,
                        doc,
                        presentation,
                        checks_unavailable,
                        failure.as_deref(),
                        activity_path,
                    );
                });
        });
    } else {
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
        // The modal owns scrolling on narrow windows. One continuous document
        // keeps the reply composer reachable without tiny nested scroll panes.
        details::paint(
            ui,
            s,
            doc,
            presentation,
            checks_unavailable,
            failure.as_deref(),
            activity_path,
        );
        ui.add_space(theme::spacing::L);
        ui.separator();
        conversation::paint(ui, s, ticket, &mut view, brief.as_ref(), height);
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
