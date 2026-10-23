use std::{collections::BTreeMap, sync::LazyLock};

static TASK_QUEUE_WAITS: LazyLock<std::sync::Mutex<BTreeMap<String, u64>>> =
    LazyLock::new(|| std::sync::Mutex::new(BTreeMap::new()));

/// Associate the task's queue wait with every remote execution record for its run.
pub fn set_task_queue_wait_ms(task_id: &str, wait_ms: Option<u64>) -> anyhow::Result<()> {
    super::validate_id(task_id)?;
    anyhow::ensure!(
        wait_ms.is_none_or(|duration| duration <= super::MAX_MS),
        "task queue wait exceeds the telemetry limit"
    );
    let mut waits = TASK_QUEUE_WAITS
        .lock()
        .map_err(|_| anyhow::anyhow!("task queue telemetry is unavailable"))?;
    if let Some(wait_ms) = wait_ms {
        waits.insert(task_id.to_owned(), wait_ms);
    } else {
        waits.remove(task_id);
    }
    Ok(())
}

pub fn clear_task_queue_wait(task_id: &str) {
    if let Ok(mut waits) = TASK_QUEUE_WAITS.lock() {
        waits.remove(task_id);
    }
}

pub(super) fn task_queue_wait_ms(task_id: &str) -> Option<u64> {
    TASK_QUEUE_WAITS.lock().ok()?.get(task_id).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_wait_is_scoped_to_task_identity_and_bounded() {
        let task_id = "00000000-0000-4000-8000-000000000061";
        let other_task = "00000000-0000-4000-8000-000000000062";
        set_task_queue_wait_ms(task_id, Some(123)).unwrap();
        assert_eq!(task_queue_wait_ms(task_id), Some(123));
        assert_eq!(task_queue_wait_ms(other_task), None);
        assert!(set_task_queue_wait_ms(task_id, Some(super::super::MAX_MS + 1)).is_err());
        clear_task_queue_wait(task_id);
        assert_eq!(task_queue_wait_ms(task_id), None);
    }
}
