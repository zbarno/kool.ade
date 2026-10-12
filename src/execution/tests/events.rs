use crate::execution::contract::{
    EventAcceptance, EventStreamValidator, ExecutionEventKindV1, ExecutionEventV1,
    ExecutionLifecycleV1,
};

fn event(sequence: u64, timestamp: u64, event: ExecutionEventKindV1) -> ExecutionEventV1 {
    ExecutionEventV1 {
        schema_version: 1,
        execution_id: uuid::Uuid::nil().hyphenated().to_string(),
        sequence,
        timestamp_unix_ms: timestamp,
        event,
    }
}

#[test]
fn event_stream_enforces_order_sequences_timestamps_and_exact_replay_deduplication() {
    let mut stream = EventStreamValidator::default();
    let queued = event(
        1,
        1,
        ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Queued,
        },
    );
    assert_eq!(stream.accept(&queued).unwrap(), EventAcceptance::Accepted);
    assert_eq!(stream.accept(&queued).unwrap(), EventAcceptance::Duplicate);
    stream
        .accept(&event(
            2,
            2,
            ExecutionEventKindV1::Lifecycle {
                state: ExecutionLifecycleV1::Preparing,
            },
        ))
        .unwrap();
    stream
        .accept(&event(
            3,
            3,
            ExecutionEventKindV1::Lifecycle {
                state: ExecutionLifecycleV1::Running,
            },
        ))
        .unwrap();
    let output = event(
        4,
        4,
        ExecutionEventKindV1::Stdout {
            chunk: "bounded output".into(),
        },
    );
    stream.accept(&output).unwrap();

    let mut changed_replay = output.clone();
    changed_replay.event = ExecutionEventKindV1::Stderr {
        chunk: "different event".into(),
    };
    assert!(stream.accept(&changed_replay).is_err());
    assert!(
        stream
            .accept(&event(
                5,
                5,
                ExecutionEventKindV1::Lifecycle {
                    state: ExecutionLifecycleV1::Succeeded,
                },
            ))
            .is_err()
    );
}

#[test]
fn event_values_round_trip_with_a_versioned_lifecycle() {
    let event = event(
        1,
        1,
        ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Queued,
        },
    );
    let encoded = serde_json::to_vec(&event).unwrap();
    let decoded: ExecutionEventV1 = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, event);
    decoded.validate().unwrap();
}

#[test]
fn event_replay_memory_is_bounded_and_old_replays_are_rejected() {
    let mut stream = EventStreamValidator::default();
    for (sequence, lifecycle) in [
        ExecutionLifecycleV1::Queued,
        ExecutionLifecycleV1::Preparing,
        ExecutionLifecycleV1::Running,
    ]
    .into_iter()
    .enumerate()
    {
        stream
            .accept(&event(
                sequence as u64 + 1,
                sequence as u64 + 1,
                ExecutionEventKindV1::Lifecycle { state: lifecycle },
            ))
            .unwrap();
    }
    let mut oldest = None;
    for sequence in 4..=300 {
        let output = event(
            sequence,
            sequence,
            ExecutionEventKindV1::Stdout { chunk: "x".into() },
        );
        if sequence == 4 {
            oldest = Some(output.clone());
        }
        stream.accept(&output).unwrap();
    }
    assert!(stream.accept(&oldest.unwrap()).is_err());
}
