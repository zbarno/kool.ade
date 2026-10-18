use super::*;

#[test]
fn execution_event_journal_is_ordered_idempotent_and_resumable() {
    use crate::execution::contract::{
        EventPageRequestV1, ExecutionEventKindV1, ExecutionEventV1, ExecutionLifecycleV1,
    };

    let root = std::env::temp_dir().join(format!(
        "koolade-execution-journal-{}",
        uuid::Uuid::new_v4()
    ));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let execution_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let event = ExecutionEventV1 {
        schema_version: 1,
        execution_id: execution_id.clone(),
        sequence: 1,
        timestamp_unix_ms: 10,
        event: ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Queued,
        },
    };
    append_event(&task_id, &event).unwrap();
    append_event(&task_id, &event).unwrap();
    assert_eq!(
        load_events(
            &task_id,
            &execution_id,
            EventPageRequestV1 {
                after_sequence: 0,
                max_events: 256,
            },
        )
        .unwrap(),
        vec![event.clone()]
    );
    assert!(
        load_events(
            &task_id,
            &execution_id,
            EventPageRequestV1 {
                after_sequence: 1,
                max_events: 256,
            },
        )
        .unwrap()
        .is_empty()
    );

    let mut conflicting = event;
    conflicting.timestamp_unix_ms += 1;
    assert!(append_event(&task_id, &conflicting).is_err());
}

#[test]
fn execution_event_journal_reports_bounded_validator_reason() {
    use crate::execution::contract::{
        EventPageRequestV1, ExecutionEventKindV1, ExecutionEventV1, ExecutionLifecycleV1,
    };

    let root = std::env::temp_dir().join(format!(
        "koolade-execution-journal-invalid-{}",
        uuid::Uuid::new_v4()
    ));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let execution_id = uuid::Uuid::new_v4().hyphenated().to_string();
    for (sequence, timestamp_unix_ms, state) in [
        (1, 10, ExecutionLifecycleV1::Queued),
        (2, 9, ExecutionLifecycleV1::Preparing),
    ] {
        append_event(
            &task_id,
            &ExecutionEventV1 {
                schema_version: 1,
                execution_id: execution_id.clone(),
                sequence,
                timestamp_unix_ms,
                event: ExecutionEventKindV1::Lifecycle { state },
            },
        )
        .unwrap();
    }

    let error = load_events(
        &task_id,
        &execution_id,
        EventPageRequestV1 {
            after_sequence: 0,
            max_events: 256,
        },
    )
    .unwrap_err();
    assert!(error.contains("event timestamp moved backwards"), "{error}");
    assert!(error.len() < 256, "{error}");
    assert!(!error.contains(root.to_str().unwrap()));
}
