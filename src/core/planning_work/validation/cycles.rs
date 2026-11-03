use super::super::Work;

pub(super) fn validate(root: &Work, work: &[Work]) -> anyhow::Result<()> {
    if root.task_intent_cycles.is_empty() {
        return Ok(());
    }
    anyhow::ensure!(
        root.task_intent_cycle_uid.as_deref()
            == root
                .task_intent_cycles
                .last()
                .map(|cycle| cycle.uid.as_str()),
        "TaskIntent current cycle must be the latest recorded cycle"
    );
    let mut cycle_uids = std::collections::BTreeSet::new();
    let mut work_uids = std::collections::BTreeSet::new();
    for (index, cycle) in root.task_intent_cycles.iter().enumerate() {
        anyhow::ensure!(
            cycle.sequence == index as u64 + 1
                && uuid::Uuid::parse_str(&cycle.uid).is_ok()
                && cycle_uids.insert(cycle.uid.as_str())
                && uuid::Uuid::parse_str(&cycle.work_uid).is_ok()
                && work_uids.insert(cycle.work_uid.as_str())
                && !cycle.goal.trim().is_empty()
                && cycle.goal.len() <= super::super::MAX_TASK_INTENT_CYCLE_GOAL_BYTES
                && !cycle
                    .goal
                    .chars()
                    .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t')),
            "TaskIntent contains an invalid work cycle"
        );
        if cycle.work_uid == root.uid {
            anyhow::ensure!(
                cycle.uid == root.uid && cycle.sequence == 1,
                "Only the initial TaskIntent cycle may use the root work record"
            );
        } else {
            let cycle_work = work
                .iter()
                .find(|item| item.uid == cycle.work_uid)
                .ok_or_else(|| anyhow::anyhow!("TaskIntent cycle work is missing"))?;
            anyhow::ensure!(
                cycle_work.task_intent_uid.as_deref() == Some(root.uid.as_str())
                    && cycle_work.task_intent_cycle_uid.as_deref() == Some(cycle.uid.as_str()),
                "TaskIntent cycle work must remain linked to its root and cycle"
            );
        }
    }
    Ok(())
}
