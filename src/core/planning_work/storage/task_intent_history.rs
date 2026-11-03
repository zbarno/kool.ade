use super::super::history::clip_event_summary;
use super::super::{TaskIntentEventKindV1, TaskIntentEventV1, Work, WorkRecord};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn append_events(
    current: &BTreeMap<String, WorkRecord>,
    desired: &mut Vec<Work>,
    complete: &mut [Work],
) -> anyhow::Result<()> {
    let desired_uids = desired
        .iter()
        .map(|work| work.uid.as_str())
        .collect::<BTreeSet<_>>();
    let mut impacted = BTreeSet::<String>::new();
    for item in complete.iter() {
        if item.task_intent_root_uid() == Some(item.uid.as_str()) {
            let previous = current.get(&item.uid);
            let root_changed = previous.is_none_or(|record| {
                desired_uids.contains(item.uid.as_str())
                    && (record.data.request != item.request
                        || record.data.status != item.status
                        || record.data.task_intent_cycles != item.task_intent_cycles
                        || record.data.task_intent_archived_at_ms
                            != item.task_intent_archived_at_ms)
            });
            let history_changed = previous
                .is_none_or(|record| record.data.task_intent_history != item.task_intent_history);
            if root_changed || history_changed {
                impacted.insert(item.uid.clone());
            }
        }
        if let Some(root_uid) = item
            .task_intent_uid
            .as_deref()
            .filter(|root_uid| *root_uid != item.uid)
        {
            let was_linked = current
                .get(&item.uid)
                .is_some_and(|record| record.data.task_intent_uid.as_deref() == Some(root_uid));
            if !was_linked {
                impacted.insert(root_uid.to_owned());
            }
        }
    }

    for root_uid in impacted {
        let linked = complete
            .iter()
            .filter(|work| {
                work.uid != root_uid && work.task_intent_uid.as_deref() == Some(&root_uid)
            })
            .filter(|work| {
                current
                    .get(&work.uid)
                    .is_none_or(|record| record.data.task_intent_uid.as_deref() != Some(&root_uid))
            })
            .map(|work| work.uid.clone())
            .collect::<Vec<_>>();
        let Some(root) = complete.iter_mut().find(|work| work.uid == root_uid) else {
            continue;
        };
        if root.task_intent_root_uid() != Some(root.uid.as_str()) {
            continue;
        }
        let previous = current.get(&root_uid).map(|record| &record.data);
        if let Some(previous) = previous {
            anyhow::ensure!(
                root.task_intent_history
                    .starts_with(&previous.task_intent_history),
                "TaskIntent history is append-only"
            );
            anyhow::ensure!(
                root.task_intent_cycles
                    .starts_with(&previous.task_intent_cycles),
                "TaskIntent work cycles are append-only"
            );
        }
        if root.task_intent_history.is_empty() {
            push_event(
                root,
                TaskIntentEventKindV1::Created,
                "TaskIntent created",
                None,
            );
        }
        if let Some(previous) = previous {
            if desired_uids.contains(root_uid.as_str()) && previous.request != root.request {
                push_event(
                    root,
                    TaskIntentEventKindV1::GoalRevised,
                    &root.request.clone(),
                    None,
                );
            }
            if desired_uids.contains(root_uid.as_str()) && previous.status != root.status {
                push_event(
                    root,
                    TaskIntentEventKindV1::StatusChanged,
                    &format!(
                        "Planning status changed from {:?} to {:?}",
                        previous.status, root.status
                    ),
                    None,
                );
            }
            let started_cycles = root
                .task_intent_cycles
                .iter()
                .skip(previous.task_intent_cycles.len())
                .map(|cycle| (cycle.sequence, cycle.goal.clone(), cycle.work_uid.clone()))
                .collect::<Vec<_>>();
            for (sequence, goal, work_uid) in started_cycles {
                push_event(
                    root,
                    TaskIntentEventKindV1::FollowUpCycleStarted,
                    &format!("Follow-up cycle {sequence} started: {goal}"),
                    Some(work_uid),
                );
            }
            match (
                previous.task_intent_archived_at_ms,
                root.task_intent_archived_at_ms,
            ) {
                (None, Some(_)) => push_event(
                    root,
                    TaskIntentEventKindV1::Archived,
                    "TaskIntent archived; its history remains available.",
                    None,
                ),
                (Some(_), None) => push_event(
                    root,
                    TaskIntentEventKindV1::Restored,
                    "TaskIntent restored from archive.",
                    None,
                ),
                (Some(previous), Some(current)) => anyhow::ensure!(
                    previous == current,
                    "TaskIntent archive timestamp is immutable until restored"
                ),
                (None, None) => {}
            }
        }
        for uid in linked {
            push_event(
                root,
                TaskIntentEventKindV1::WorkLinked,
                "Planning work linked to this TaskIntent",
                Some(uid),
            );
        }
    }

    let mut by_uid = complete
        .iter()
        .map(|work| (work.uid.as_str(), work))
        .collect::<BTreeMap<_, _>>();
    for item in desired.iter_mut() {
        if let Some(updated) = by_uid.remove(item.uid.as_str()) {
            *item = updated.clone();
        }
    }
    for (uid, item) in by_uid {
        if current.contains_key(uid)
            && item.task_intent_root_uid() == Some(item.uid.as_str())
            && item.task_intent_history
                != current
                    .get(uid)
                    .expect("checked current record")
                    .data
                    .task_intent_history
        {
            desired.push(item.clone());
        }
    }
    Ok(())
}

fn push_event(
    root: &mut Work,
    kind: TaskIntentEventKindV1,
    summary: &str,
    related_uid: Option<String>,
) {
    let previous = root
        .task_intent_history
        .last()
        .map_or(0, |event| event.recorded_at_ms);
    root.task_intent_history.push(TaskIntentEventV1 {
        event_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        kind,
        recorded_at_ms: super::legacy::now_ms().max(previous.saturating_add(1)),
        summary: clip_event_summary(summary).to_owned(),
        cycle_uid: root.task_intent_cycle_uid.clone(),
        related_uid,
        evidence: None,
    });
}
