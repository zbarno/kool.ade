//! Exercise completed-history quota policy and the production record/journal boundaries.
//! The concurrent API test protects the shared lock invariant that unit tests of pruning alone
//! cannot establish; it uses only temporary local state, with no Docker or network setup.
use super::*;

#[path = "retention/bootstrap.rs"]
mod bootstrap;

#[test]
fn completed_history_retention_prunes_oldest_record_and_its_event_directory() {
    let root = std::env::temp_dir().join(format!(
        "koolade-execution-retention-{}",
        uuid::Uuid::new_v4()
    ));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let mut paths = Vec::new();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let owner = owner_instance_id().unwrap();
    for updated_at in [10, 20, 30] {
        let mut completed = record();
        completed.task_id = task_id.clone();
        completed.owner_instance_id = owner.clone();
        completed.updated_at_unix_ms = updated_at;
        completed.started_at_unix_ms = updated_at;
        completed.ended_at_unix_ms = Some(updated_at);
        completed.state = ExecutionRecordStateV1::Succeeded;
        completed.cleanup = ExecutionCleanupStateV1::Complete;
        save(&completed).unwrap();
        let task_dir = root.join("executions").join(&task_id);
        let journal = task_dir.join(format!("{}.events", completed.execution_id));
        fs::create_dir(&journal).unwrap();
        paths.push((
            task_dir.join(format!("{}.json", completed.execution_id)),
            journal,
        ));
    }

    super::super::super::retention::prune_completed_with_limits(
        &root.join("executions"),
        2,
        u64::MAX,
        &owner,
        None,
    )
    .unwrap();

    assert!(!paths[0].0.exists());
    assert!(!paths[0].1.exists());
    assert!(paths[1].0.exists() && paths[1].1.exists());
    assert!(paths[2].0.exists() && paths[2].1.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn completed_history_retention_enforces_aggregate_byte_quota() {
    let root = std::env::temp_dir().join(format!(
        "koolade-execution-retention-bytes-{}",
        uuid::Uuid::new_v4()
    ));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let owner = owner_instance_id().unwrap();
    let mut completed = record();
    completed.owner_instance_id = owner.clone();
    completed.ended_at_unix_ms = Some(completed.updated_at_unix_ms);
    completed.state = ExecutionRecordStateV1::Succeeded;
    completed.cleanup = ExecutionCleanupStateV1::Complete;
    let record_path = root
        .join("executions")
        .join(&completed.task_id)
        .join(format!("{}.json", completed.execution_id));
    save(&completed).unwrap();

    super::super::super::retention::prune_completed_with_limits(
        &root.join("executions"),
        10,
        0,
        &owner,
        None,
    )
    .unwrap();

    assert!(!record_path.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn retention_is_owner_scoped_and_ties_prune_by_execution_identity() {
    let root = std::env::temp_dir().join(format!(
        "koolade-execution-retention-owner-{}",
        uuid::Uuid::new_v4()
    ));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let owner = owner_instance_id().unwrap();
    let foreign_owner = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut tied = [record(), record()];
    for completed in &mut tied {
        completed.task_id = task_id.clone();
        completed.owner_instance_id = owner.clone();
        completed.updated_at_unix_ms = 100;
        completed.ended_at_unix_ms = Some(100);
        completed.state = ExecutionRecordStateV1::Succeeded;
        completed.cleanup = ExecutionCleanupStateV1::Complete;
        save(completed).unwrap();
    }
    let mut foreign = record();
    foreign.owner_instance_id = foreign_owner;
    foreign.updated_at_unix_ms = 50;
    foreign.ended_at_unix_ms = Some(50);
    foreign.state = ExecutionRecordStateV1::Succeeded;
    foreign.cleanup = ExecutionCleanupStateV1::Complete;
    save(&foreign).unwrap();

    let expected_pruned = tied
        .iter()
        .min_by_key(|record| record.execution_id.as_str())
        .unwrap();
    super::super::super::retention::prune_completed_with_limits(
        &root.join("executions"),
        1,
        u64::MAX,
        &owner,
        None,
    )
    .unwrap();

    assert!(
        load(&expected_pruned.task_id, &expected_pruned.execution_id)
            .unwrap()
            .is_none()
    );
    let other_tied = tied
        .iter()
        .find(|record| record.execution_id != expected_pruned.execution_id)
        .unwrap();
    assert!(
        load(&other_tied.task_id, &other_tied.execution_id)
            .unwrap()
            .is_some()
    );
    assert!(
        load(&foreign.task_id, &foreign.execution_id)
            .unwrap()
            .is_some()
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn retention_does_not_prune_a_task_while_its_task_lock_is_held() {
    let root = std::env::temp_dir().join(format!(
        "koolade-execution-retention-task-lock-{}",
        uuid::Uuid::new_v4()
    ));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let owner = owner_instance_id().unwrap();
    let mut completed = record();
    completed.owner_instance_id = owner.clone();
    completed.ended_at_unix_ms = Some(completed.updated_at_unix_ms);
    completed.state = ExecutionRecordStateV1::Succeeded;
    completed.cleanup = ExecutionCleanupStateV1::Complete;
    save(&completed).unwrap();
    let task_lock = try_task_lock(&completed.task_id).unwrap().unwrap();

    assert!(
        super::super::super::retention::prune_completed_with_limits(
            &root.join("executions"),
            0,
            u64::MAX,
            &owner,
            None,
        )
        .is_err()
    );
    assert!(
        load(&completed.task_id, &completed.execution_id)
            .unwrap()
            .is_some()
    );
    drop(task_lock);
    super::super::super::retention::prune_completed_with_limits(
        &root.join("executions"),
        0,
        u64::MAX,
        &owner,
        None,
    )
    .unwrap();
    assert!(
        load(&completed.task_id, &completed.execution_id)
            .unwrap()
            .is_none()
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn completed_record_write_and_prune_share_one_retention_lock() {
    use std::sync::mpsc;
    use std::time::Duration;

    let root = std::env::temp_dir().join(format!(
        "koolade-execution-retention-lock-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let lock_path = root.join("retention.lock");
    let (first_entered_tx, first_entered_rx) = mpsc::channel();
    let (release_first_tx, release_first_rx) = mpsc::channel();
    let first_lock = lock_path.clone();
    let first = std::thread::spawn(move || {
        super::super::super::retention::with_retention_lock_path(&first_lock, || {
            first_entered_tx.send(()).unwrap();
            release_first_rx.recv().unwrap();
            Ok(())
        })
        .unwrap();
    });
    first_entered_rx.recv().unwrap();

    let (second_attempting_tx, second_attempting_rx) = mpsc::channel();
    let (second_entered_tx, second_entered_rx) = mpsc::channel();
    let second_lock = lock_path.clone();
    let second = std::thread::spawn(move || {
        second_attempting_tx.send(()).unwrap();
        super::super::super::retention::with_retention_lock_path(&second_lock, || {
            second_entered_tx.send(()).unwrap();
            Ok(())
        })
        .unwrap();
    });
    second_attempting_rx.recv().unwrap();
    assert!(
        second_entered_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err()
    );

    release_first_tx.send(()).unwrap();
    second_entered_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    first.join().unwrap();
    second.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn concurrent_production_saves_and_journal_appends_remain_prunable() {
    use crate::execution::contract::{
        ExecutionEventKindV1, ExecutionEventV1, ExecutionLifecycleV1,
    };

    let root = std::env::temp_dir().join(format!(
        "koolade-execution-concurrent-retention-{}",
        uuid::Uuid::new_v4()
    ));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let owner = owner_instance_id().unwrap();
    let workers = (0..8)
        .map(|_| {
            let owner = owner.clone();
            std::thread::spawn(move || {
                let mut record = record();
                record.owner_instance_id = owner;
                save(&record).unwrap();
                append_event(
                    &record.task_id,
                    &ExecutionEventV1 {
                        schema_version: 1,
                        execution_id: record.execution_id.clone(),
                        sequence: 1,
                        timestamp_unix_ms: record.started_at_unix_ms,
                        event: ExecutionEventKindV1::Lifecycle {
                            state: ExecutionLifecycleV1::Queued,
                        },
                    },
                )
                .unwrap();
                record.state = ExecutionRecordStateV1::Succeeded;
                record.updated_at_unix_ms = record.started_at_unix_ms + 1;
                record.ended_at_unix_ms = Some(record.updated_at_unix_ms);
                record.cleanup = ExecutionCleanupStateV1::Complete;
                save(&record).unwrap();
                record
            })
        })
        .collect::<Vec<_>>();

    for worker in workers {
        let record = worker.join().unwrap();
        assert_eq!(
            load(&record.task_id, &record.execution_id).unwrap(),
            Some(record)
        );
    }
    let _ = fs::remove_dir_all(root);
}
