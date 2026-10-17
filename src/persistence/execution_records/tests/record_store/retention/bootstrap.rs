//! Verify the first production save and journal append bootstrap a fresh application state root.
//! A pure validator test cannot cover directory creation and lock setup; this uses temporary local
//! state only, with low setup cost and no Docker or network dependency.
use super::*;

#[test]
fn first_save_and_append_bootstrap_a_fresh_application_state_root() {
    use crate::execution::contract::{
        ExecutionEventKindV1, ExecutionEventV1, ExecutionLifecycleV1,
    };

    let root = std::env::temp_dir().join(format!(
        "koolade-execution-fresh-state-{}",
        uuid::Uuid::new_v4()
    ));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let record = record();
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

    assert!(
        load(&record.task_id, &record.execution_id)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        journal_cursor(&record.task_id, &record.execution_id).unwrap(),
        1
    );
    let _ = fs::remove_dir_all(root);
}
