use super::super::super::*;
use crate::ui::Surface;
use std::cmp::Reverse;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    WaitingOnUser,
    Blocked,
}

enum Entry<'a> {
    Work(&'a crate::core::planning_work::Work),
    Setup(&'a crate::app::setup_attention::SetupIssue),
    Item(&'a crate::domain::item::OpenItem),
    Task(&'a crate::artifacts::task_docs::TaskDocument),
}

pub(super) struct Lane<'a> {
    pub planning: &'a [&'a crate::core::planning_work::Work],
    pub issue: Option<&'a crate::app::setup_attention::SetupIssue>,
    pub items: &'a [&'a crate::domain::item::OpenItem],
    pub tasks: &'a [&'a crate::artifacts::task_docs::TaskDocument],
}

pub(super) fn badge(ui: &mut egui::Ui, kind: Kind) {
    let (label, color) = match kind {
        Kind::WaitingOnUser => ("Waiting on user", theme::WARNING),
        Kind::Blocked => ("Blocked", theme::TEXT_DIM),
    };
    theme::badge(ui, label, color.gamma_multiply(0.14), color);
}

pub(super) fn user_action(ui: &mut egui::Ui, action: &str) {
    let (color, fill) = user_action_colors(ui.visuals().dark_mode);
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.5, color))
        .corner_radius(6)
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new("NEEDS YOUR INPUT")
                        .strong()
                        .color(color),
                );
                ui.label(egui::RichText::new(action).strong());
            });
        });
}

fn user_action_colors(dark_mode: bool) -> (egui::Color32, egui::Color32) {
    if dark_mode {
        (theme::WARNING, theme::WARNING.gamma_multiply(0.12))
    } else {
        (
            egui::Color32::from_rgb(142, 93, 0),
            egui::Color32::from_rgb(255, 245, 205),
        )
    }
}

pub(super) fn linked_user_action<'a>(
    board: &'a crate::ui::planning_board::ViewModel,
    task_path: &str,
) -> Option<&'a crate::domain::item::OpenItem> {
    let feature_id = feature_id_in_task_path(task_path)?;
    board.planning_items.iter().find(|item| {
        item.status == crate::domain::ItemStatus::Open
            && item.feature_id.as_deref() == Some(feature_id.as_str())
            && board.eligible_item_ids.contains(&item.id)
    })
}

fn feature_id_in_task_path(path: &str) -> Option<String> {
    let directory = path.split('/').rev().nth(1)?;
    let candidate = if let Some(rest) = directory.strip_prefix("CHG-") {
        format!("CHG-{}", rest.split_once('-')?.0)
    } else {
        directory.split_once('-')?.0.to_owned()
    };
    crate::artifacts::product_docs::valid_feature_id(&candidate).then_some(candidate)
}

fn recovery_kind(recovery: crate::core::implementation::RecoveryDisposition) -> Kind {
    match recovery {
        crate::core::implementation::RecoveryDisposition::UserAction
        | crate::core::implementation::RecoveryDisposition::ExplicitResume => Kind::WaitingOnUser,
        crate::core::implementation::RecoveryDisposition::AutomaticRetry
        | crate::core::implementation::RecoveryDisposition::DoNotRetry => Kind::Blocked,
    }
}

pub(super) fn task_kind(s: &dyn Surface, key: &str) -> Kind {
    if let Some(recovery) = s.implementation_recovery(key) {
        return recovery_kind(recovery);
    }
    if s.implementation_state(key).is_some_and(|state| {
        state.pr_state == Some(crate::core::implementation::PullRequestState::Closed)
    }) {
        return Kind::WaitingOnUser;
    }
    match s.implementation_state(key).map(|state| state.status) {
        Some(
            crate::core::implementation::ImplementationStatus::WaitingToMerge
            | crate::core::implementation::ImplementationStatus::WaitingForIndependentChecks
            | crate::core::implementation::ImplementationStatus::AwaitingReview
            | crate::core::implementation::ImplementationStatus::Blocked,
        ) => Kind::Blocked,
        Some(
            crate::core::implementation::ImplementationStatus::ReadyToPublish
            | crate::core::implementation::ImplementationStatus::AwaitingApproval
            | crate::core::implementation::ImplementationStatus::ChangesRequested
            | crate::core::implementation::ImplementationStatus::PullRequestClosed
            | crate::core::implementation::ImplementationStatus::Interrupted,
        ) => Kind::WaitingOnUser,
        _ => Kind::WaitingOnUser,
    }
}

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    lane: Lane<'_>,
    selected_path: &mut Option<String>,
    planning_selection: &mut Option<String>,
) {
    let mut entries = Vec::new();
    let mut fallback_order = 0usize;
    for work in lane.planning {
        entries.push((
            Entry::Work(work),
            last_message_ms(s, &work.key),
            fallback_order,
        ));
        fallback_order += 1;
    }
    if let Some(issue) = lane.issue {
        entries.push((Entry::Setup(issue), None, fallback_order));
        fallback_order += 1;
    }
    for item in lane.items {
        let recency = [
            last_message_ms(s, item.conversation_key()),
            last_message_ms(s, &item.id),
        ]
        .into_iter()
        .flatten()
        .max();
        entries.push((Entry::Item(item), recency, fallback_order));
        fallback_order += 1;
    }
    for task in lane.tasks {
        entries.push((
            Entry::Task(task),
            task_activity_ms(s, &task.path),
            fallback_order,
        ));
        fallback_order += 1;
    }
    sort_recency(&mut entries);
    for (entry, _, _) in entries {
        match entry {
            Entry::Work(work) => super::cards::work(ui, s, work, lane.planning, 3),
            Entry::Setup(issue) => super::cards::setup(ui, s, board, issue),
            Entry::Item(item) => super::cards::item(ui, s, board, item, 3, planning_selection),
            Entry::Task(task) => super::cards::task(ui, s, board, task, 3, selected_path),
        }
    }
}

fn sort_recency<T>(entries: &mut [(T, Option<i64>, usize)]) {
    entries.sort_by_key(|entry| (Reverse(entry.1), entry.2));
}

fn last_message_ms(s: &dyn Surface, key: &str) -> Option<i64> {
    let task_message = s
        .task_messages(key)
        .iter()
        .map(|message| message.ts.timestamp_millis())
        .max();
    let main_message = s
        .chat_messages()
        .iter()
        .filter(|message| message.ref_item.as_deref() == Some(key))
        .map(|message| message.ts.timestamp_millis())
        .max();
    [task_message, main_message].into_iter().flatten().max()
}

fn task_activity_ms(s: &dyn Surface, key: &str) -> Option<i64> {
    let progress_time = s.task_progress(key).and_then(progress_activity_ms);
    let pr_time = s
        .implementation_state(key)
        .and_then(|state| state.pr_checked_at.as_deref())
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|date| date.timestamp_millis());
    [last_message_ms(s, key), progress_time, pr_time]
        .into_iter()
        .flatten()
        .max()
}

fn progress_activity_ms(progress: &crate::harness::LiveProgress) -> Option<i64> {
    [
        progress.telemetry.started_ms,
        progress.telemetry.updated_ms,
        progress.telemetry.finished_ms,
    ]
    .into_iter()
    .flatten()
    .max()
}

#[cfg(test)]
#[path = "attention_tests.rs"]
mod tests;
