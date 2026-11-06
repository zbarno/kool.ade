use crate::domain::{ChatMessage, ChatRole};
use crate::harness::{LiveProgress, TaskActivityActor, TaskActivityOrigin};
use egui::RichText;
use std::cmp::Ordering;

#[path = "timeline/entry.rs"]
mod entry;
use entry::paint_entry;

#[cfg(test)]
#[path = "timeline/tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Conversation,
    Activity,
}

struct Entry {
    id: String,
    at_ms: i64,
    category: Filter,
    sequence: Option<u64>,
    actor: &'static str,
    kind: String,
    status: Option<String>,
    run: Option<u32>,
    origin: Option<&'static str>,
    text: String,
}

pub(super) fn paint(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    progress: Option<&LiveProgress>,
    busy: bool,
    ticket: &str,
    height: f32,
) {
    let filter_id = egui::Id::new(("task_timeline_filter", ticket));
    let follow_id = egui::Id::new(("task_timeline_follow", ticket));
    let limit_id = egui::Id::new(("task_timeline_limit", ticket));
    let mut filter = ui
        .ctx()
        .data_mut(|data| data.get_temp::<Filter>(filter_id).unwrap_or(Filter::All));
    let mut follow = ui
        .ctx()
        .data_mut(|data| data.get_temp::<bool>(follow_id).unwrap_or(true));
    let mut visible_limit = ui
        .ctx()
        .data_mut(|data| data.get_temp::<usize>(limit_id).unwrap_or(120));
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Task timeline").strong());
        ui.selectable_value(&mut filter, Filter::All, "All");
        ui.selectable_value(&mut filter, Filter::Conversation, "Conversation");
        ui.selectable_value(&mut filter, Filter::Activity, "Activity");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.toggle_value(&mut follow, "Follow live");
        });
    });
    ui.ctx().data_mut(|data| {
        data.insert_temp(filter_id, filter);
        data.insert_temp(follow_id, follow);
    });
    let mut entries = entries(messages, progress);
    entries.sort_by(chronological_order);
    let matching = entries
        .into_iter()
        .filter(|entry| match filter {
            Filter::All => true,
            Filter::Conversation => entry.category == Filter::Conversation,
            Filter::Activity => entry.category == Filter::Activity,
        })
        .collect::<Vec<_>>();
    let has_older = matching.len() > visible_limit;
    if has_older && ui.small_button("Load older history").clicked() {
        visible_limit = visible_limit.saturating_add(100).min(2000);
        ui.ctx()
            .data_mut(|data| data.insert_temp(limit_id, visible_limit));
    }
    let truncated = progress
        .and_then(|progress| progress.activity_events_truncated_before_ms)
        .is_some();
    egui::ScrollArea::vertical()
        .id_salt(("task_modal_timeline", ticket))
        .max_height(height)
        .auto_shrink([false, false])
        .stick_to_bottom(follow)
        .show(ui, |ui| {
            if truncated {
                ui.label(
                    RichText::new(
                        "Older worker activity was trimmed; the recent event window is shown.",
                    )
                    .small()
                    .weak(),
                );
            }
            if matching.is_empty() {
                ui.label(if busy {
                    "Waiting for the first task update…"
                } else {
                    "No task conversation or activity recorded yet."
                });
                return;
            }
            for entry in matching
                .iter()
                .skip(matching.len().saturating_sub(visible_limit))
            {
                paint_entry(ui, entry);
            }
        });
}

fn entries(messages: &[ChatMessage], progress: Option<&LiveProgress>) -> Vec<Entry> {
    let mut entries = messages
        .iter()
        .map(|message| {
            let actor = match message.role {
                ChatRole::User => "You",
                ChatRole::Agent => "Manager",
                ChatRole::System => "System",
            };
            Entry {
                id: format!("chat:{}", message.id),
                at_ms: message.ts.timestamp_millis(),
                category: Filter::Conversation,
                sequence: None,
                actor,
                kind: "Conversation".into(),
                status: None,
                run: None,
                origin: None,
                text: crate::ui::message_text::readable(message).to_string(),
            }
        })
        .collect::<Vec<_>>();
    let Some(progress) = progress else {
        return entries;
    };
    entries.extend(
        progress
            .activity_events
            .iter()
            .filter(|event| !event.kind.eq_ignore_ascii_case("thinking"))
            .map(|event| Entry {
                id: event.event_id.clone(),
                at_ms: event.recorded_at_ms,
                category: Filter::Activity,
                sequence: Some(event.sequence),
                actor: match event.actor {
                    TaskActivityActor::Worker => "Worker",
                    TaskActivityActor::System => "System",
                    TaskActivityActor::Reviewer => "Reviewer",
                },
                kind: event.kind.clone(),
                status: event.status.clone(),
                run: Some(event.attempt),
                origin: Some(match event.origin {
                    TaskActivityOrigin::ProviderObserved => "Observed",
                    TaskActivityOrigin::ApplicationInferred => "Application status",
                }),
                text: event.content.clone(),
            }),
    );
    if !progress.response.trim().is_empty()
        && !progress
            .activity_events
            .iter()
            .any(|event| event.content == progress.response)
    {
        entries.push(Entry {
            id: format!("result:{}", progress.run_id.as_deref().unwrap_or("legacy")),
            at_ms: progress
                .telemetry
                .finished_ms
                .or(progress.telemetry.updated_ms)
                .unwrap_or_default(),
            category: Filter::Activity,
            sequence: None,
            actor: "Worker",
            kind: "Result".into(),
            status: None,
            run: (progress.run_attempt > 0).then_some(progress.run_attempt),
            origin: Some("Observed"),
            text: progress.response.clone(),
        });
    }
    // Older snapshots and partially upgraded histories may have provider posts
    // without a matching typed event. Keep those visible while avoiding dupes.
    entries.extend(
        progress
            .posts
            .iter()
            .filter(|post| {
                !post.kind.eq_ignore_ascii_case("thinking")
                    && !progress.activity_events.iter().any(|event| {
                        event.content == crate::harness::safe_activity_text(&post.text)
                    })
            })
            .map(|post| Entry {
                id: format!("legacy:{}:{}", post.id.0, post.id.1),
                at_ms: post
                    .recorded_at_ms
                    .or(progress.telemetry.updated_ms)
                    .unwrap_or_default(),
                category: Filter::Activity,
                sequence: None,
                actor: "Worker",
                kind: post.kind.clone(),
                status: None,
                run: None,
                origin: Some("Legacy observed"),
                text: crate::harness::safe_activity_text(&post.text),
            }),
    );
    if let Some(activity) = progress
        .activity
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        && !entries.iter().any(|entry| entry.text == activity)
    {
        entries.push(Entry {
            id: "current-status".into(),
            at_ms: progress.telemetry.updated_ms.unwrap_or_default(),
            category: Filter::Activity,
            sequence: None,
            actor: "System",
            kind: "Current status".into(),
            status: None,
            run: None,
            origin: None,
            text: crate::harness::safe_activity_text(activity),
        });
    }
    entries
}

fn chronological_order(left: &Entry, right: &Entry) -> Ordering {
    left.at_ms
        .cmp(&right.at_ms)
        .then_with(|| {
            left.sequence
                .unwrap_or(u64::MAX)
                .cmp(&right.sequence.unwrap_or(u64::MAX))
        })
        .then_with(|| left.id.cmp(&right.id))
}
