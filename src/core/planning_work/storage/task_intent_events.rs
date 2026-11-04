use super::super::{TaskIntentEventKindV1, TaskIntentEventV1};

pub fn linked_executable_task_changes(
    store: &crate::artifacts::planning_store::PlanningStore,
    root_uid: &str,
    task_uids: &[String],
) -> anyhow::Result<crate::artifacts::planning_store::RecordChangePlan> {
    let mut work = super::load(store)?;
    let root = work
        .iter_mut()
        .find(|item| item.uid == root_uid && item.task_intent_root_uid() == Some(root_uid))
        .ok_or_else(|| anyhow::anyhow!("TaskIntent root is unavailable for task history"))?;
    for task_uid in task_uids {
        anyhow::ensure!(
            uuid::Uuid::parse_str(task_uid).is_ok(),
            "Executable task link has an invalid UID"
        );
        if root.task_intent_history.iter().any(|event| {
            event.kind == TaskIntentEventKindV1::ExecutableTaskLinked
                && event.related_uid.as_deref() == Some(task_uid)
        }) {
            continue;
        }
        let previous_time = root
            .task_intent_history
            .last()
            .map_or(0, |event| event.recorded_at_ms);
        root.task_intent_history.push(TaskIntentEventV1 {
            event_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            kind: TaskIntentEventKindV1::ExecutableTaskLinked,
            recorded_at_ms: super::now_ms().max(previous_time.saturating_add(1)),
            summary: "Executable task linked to this TaskIntent".into(),
            cycle_uid: root.task_intent_cycle_uid.clone(),
            related_uid: Some(task_uid.clone()),
            evidence: None,
        });
    }
    let root = root.clone();
    super::record_changes(store, &[root])
}
