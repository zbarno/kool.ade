use super::*;

#[test]
fn execution_records_validate_and_round_trip_outside_the_repository() {
    let root =
        std::env::temp_dir().join(format!("koolade-execution-record-{}", uuid::Uuid::new_v4()));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let mut record = record();
    let owner = owner_instance_id().unwrap();
    assert_eq!(owner, owner_instance_id().unwrap());
    record.owner_instance_id = owner.clone();
    let task_lock = try_task_lock(&record.task_id).unwrap().unwrap();
    assert!(try_task_lock(&record.task_id).unwrap().is_none());
    drop(task_lock);
    assert!(try_task_lock(&record.task_id).unwrap().is_some());
    save(&record).unwrap();
    assert_eq!(unfinished_for_owner(&owner).unwrap(), vec![record.clone()]);
    assert_eq!(
        load(&record.task_id, &record.execution_id).unwrap(),
        Some(record.clone())
    );
    assert!(root.join("executions").join(&record.task_id).exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn latest_task_execution_record_uses_persisted_update_order() {
    let root =
        std::env::temp_dir().join(format!("koolade-execution-latest-{}", uuid::Uuid::new_v4()));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let mut earlier = record();
    earlier.updated_at_unix_ms = 20;
    earlier.started_at_unix_ms = 10;
    let mut later = record();
    later.task_id = earlier.task_id.clone();
    later.updated_at_unix_ms = 30;
    later.started_at_unix_ms = 25;
    save(&earlier).unwrap();
    save(&later).unwrap();

    assert_eq!(latest_for_task(&earlier.task_id).unwrap(), Some(later));
    let _ = fs::remove_dir_all(root);
}

#[path = "record_store/retention.rs"]
mod retention;
