use super::{Work, WorkStatus};
use serde::{Deserialize, Serialize};

pub const MAX_TASK_INTENT_CYCLE_GOAL_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskIntentCycleV1 {
    pub uid: String,
    pub sequence: u64,
    pub goal: String,
    pub work_uid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
}

pub fn start_follow_up_cycle(
    work: &mut Vec<Work>,
    root_uid: &str,
    goal: &str,
) -> anyhow::Result<String> {
    let goal = goal.trim();
    anyhow::ensure!(
        !goal.is_empty() && goal.len() <= MAX_TASK_INTENT_CYCLE_GOAL_BYTES,
        "Follow-up cycle goal must be between 1 and {MAX_TASK_INTENT_CYCLE_GOAL_BYTES} UTF-8 bytes"
    );
    anyhow::ensure!(
        !goal
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t')),
        "Follow-up cycle goal contains unsupported control characters"
    );
    super::normalize_task_intents(work);
    let root_index = work
        .iter()
        .position(|item| item.uid == root_uid)
        .ok_or_else(|| anyhow::anyhow!("TaskIntent root does not exist"))?;
    let root = &work[root_index];
    anyhow::ensure!(
        root.parent_uid.is_none()
            && root.task_intent_root_uid() == Some(root.uid.as_str())
            && root.kind.starts_task_intent(),
        "Follow-up cycles require a TaskIntent root"
    );
    let prior = root
        .task_intent_cycles
        .last()
        .ok_or_else(|| anyhow::anyhow!("TaskIntent has no current work cycle"))?;
    let sequence = prior
        .sequence
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("TaskIntent cycle sequence is exhausted"))?;
    let cycle_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let work_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut cycle_work = Work::new(
        format!("task:{work_uid}"),
        format!(
            "Follow-up {sequence}: {}",
            crate::core::context_build::clip(goal, 72)
        ),
        goal.to_owned(),
        format!("Planning follow-up cycle {sequence}"),
    );
    cycle_work.uid = work_uid.clone();
    cycle_work.key = format!("task:{work_uid}");
    cycle_work.kind = root.kind;
    cycle_work.status = WorkStatus::InProgress;
    cycle_work.feature_id = None;
    cycle_work.feature_uid = None;
    cycle_work.parent_uid = Some(root_uid.to_owned());
    cycle_work.task_intent_uid = Some(root_uid.to_owned());
    cycle_work.task_intent_cycle_uid = Some(cycle_uid.clone());
    cycle_work.source_branch = root.source_branch.clone();
    cycle_work.destination_branch = root.destination_branch.clone();
    cycle_work.routing_overrides = root.routing_overrides.clone();
    cycle_work.routing_inherited_from = Some(root_uid.to_owned());

    let root = &mut work[root_index];
    root.task_intent_cycles.push(TaskIntentCycleV1 {
        uid: cycle_uid.clone(),
        sequence,
        goal: goal.to_owned(),
        work_uid,
        started_at_ms: Some(now_ms()),
    });
    root.task_intent_cycle_uid = Some(cycle_uid.clone());
    root.task_intent_archived_at_ms = None;
    work.push(cycle_work);
    Ok(cycle_uid)
}

pub(super) fn ensure_initial_cycle(root: &mut Work) {
    if !root.task_intent_cycles.is_empty() {
        root.task_intent_cycle_uid.get_or_insert_with(|| {
            root.task_intent_cycles
                .last()
                .expect("non-empty")
                .uid
                .clone()
        });
        return;
    }
    let uid = root.uid.clone();
    root.task_intent_cycles.push(TaskIntentCycleV1 {
        uid: uid.clone(),
        sequence: 1,
        goal: root
            .original_goal
            .as_deref()
            .unwrap_or(&root.request)
            .to_owned(),
        work_uid: uid.clone(),
        started_at_ms: None,
    });
    root.task_intent_cycle_uid = Some(uid);
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            elapsed.as_millis().min(u64::MAX as u128) as u64
        })
}
