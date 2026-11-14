use crate::artifacts::planning_store::PlanningStore;
use crate::core::lifecycle::{LifecycleStore, TaskEvent, TaskPhase};

pub(in crate::core::implementation) fn record_implementation_finished(
    planning_store: &PlanningStore,
    planning_item_uid: &str,
    code_sha256: &str,
) -> anyhow::Result<()> {
    let store = LifecycleStore::new(planning_store);
    let Some(mut task) = store.task_for_planning_item(planning_item_uid)? else {
        return Ok(());
    };
    if task.implementation_sha256.as_deref() == Some(code_sha256) {
        return Ok(());
    }
    let task_namespace = uuid::Uuid::parse_str(&task.task_uid)?;
    let event_uid = |stage: &str| {
        uuid::Uuid::new_v5(
            &task_namespace,
            format!("implementation-progress:v1:{stage}:{code_sha256}").as_bytes(),
        )
        .hyphenated()
        .to_string()
    };
    if task.phase == TaskPhase::Ready {
        task = store.apply_task_event(
            &task.task_uid,
            task.revision,
            &event_uid("start"),
            TaskEvent::Start,
        )?;
    }
    if task.phase == TaskPhase::Repairing {
        task = store.apply_task_event(
            &task.task_uid,
            task.revision,
            &event_uid("repair"),
            TaskEvent::BeginRepair,
        )?;
    }
    if task.phase == TaskPhase::Running {
        store.apply_task_event(
            &task.task_uid,
            task.revision,
            &event_uid("finished"),
            TaskEvent::ImplementationFinished {
                code_sha256: code_sha256.to_owned(),
            },
        )?;
        return Ok(());
    }
    anyhow::bail!(
        "verified implementation cannot advance lifecycle task from {:?} with a different code hash",
        task.phase
    )
}
