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

pub(super) fn item_kind(user_can_act: bool) -> Kind {
    if user_can_act {
        Kind::WaitingOnUser
    } else {
        Kind::Blocked
    }
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
