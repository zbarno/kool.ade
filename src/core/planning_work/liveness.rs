use super::{Work, WorkKind, WorkStatus};

/// Build one stable #568 task for a TaskIntent cycle whose dispatcher has no
/// active operation or eligible child work. The task records the diagnosis in
/// private planning state and keeps the original user goal intact.
pub fn liveness_recovery_task(root: &Work, cycle_uid: &str, diagnosis: &str) -> Work {
    let key = format!("task:manager-liveness:{}:{cycle_uid}", root.uid);
    let mut task = Work::new(
        key.clone(),
        format!(
            "Recover stalled work: {}",
            crate::core::context_build::clip(&root.title, 96)
        ),
        format!(
            "Inspect the current TaskIntent records for {} and resume the exact eligible operation if it remains authorized. If no safe operation is eligible, record the specific blocker and the next supported resolution. Do not change the approved scope or infer approval from narrative.",
            root.title
        ),
        crate::core::context_build::clip_bytes(diagnosis, 2048),
    );
    task.uid = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, key.as_bytes())
        .hyphenated()
        .to_string();
    task.key = key;
    task.kind = WorkKind::Question;
    task.status = WorkStatus::Todo;
    task.liveness_recovery_attempts = 0;
    task.original_goal = root
        .original_goal
        .clone()
        .or_else(|| Some(root.request.clone()));
    task.feature_id = root.feature_id.clone();
    task.feature_uid = root.feature_uid.clone();
    task.parent_uid = Some(root.uid.clone());
    task.task_intent_uid = Some(root.uid.clone());
    task.task_intent_cycle_uid = Some(cycle_uid.to_owned());
    task.routing_overrides = root.routing_overrides.clone();
    task.routing_inherited_from = Some(root.uid.clone());
    task
}
