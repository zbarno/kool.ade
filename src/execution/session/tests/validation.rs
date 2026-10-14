use super::*;

#[test]
fn invalid_event_stream_cancels_and_releases_the_container() {
    let backend = backend();
    backend.events.lock().unwrap()[3].sequence = 99;
    let session = session(backend.clone());
    assert!(
        session
            .run_bounded_script("true", Duration::from_secs(10))
            .is_err()
    );
    assert!(backend.cancelled.load(Ordering::Acquire));
    assert!(backend.released.load(Ordering::Acquire));
}

#[test]
fn output_over_limit_cancels_and_releases_the_container() {
    let backend = backend();
    let mut events = vec![
        event(
            1,
            ExecutionEventKindV1::Lifecycle {
                state: ExecutionLifecycleV1::Queued,
            },
        ),
        event(
            2,
            ExecutionEventKindV1::Lifecycle {
                state: ExecutionLifecycleV1::Preparing,
            },
        ),
        event(
            3,
            ExecutionEventKindV1::Lifecycle {
                state: ExecutionLifecycleV1::Running,
            },
        ),
    ];
    for sequence in 4..=21 {
        events.push(event(
            sequence,
            ExecutionEventKindV1::Stdout {
                chunk: "x".repeat(60 * 1024),
            },
        ));
    }
    events.push(event(
        22,
        ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Failed,
        },
    ));
    *backend.events.lock().unwrap() = events;

    let session = session(backend.clone());
    let error = session
        .run_bounded_script("yes", Duration::from_secs(10))
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::OutputLimitExceeded);
    assert!(backend.cancelled.load(Ordering::Acquire));
    assert!(backend.released.load(Ordering::Acquire));
}

#[test]
fn foreign_execution_event_fails_closed_instead_of_repolling() {
    let backend = backend();
    backend.events.lock().unwrap()[3].execution_id = "00000000-0000-4000-8000-000000000099".into();
    let error = session(backend.clone())
        .run_bounded_script("true", Duration::from_secs(10))
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::WorkerDisconnected);
    assert!(backend.cancelled.load(Ordering::Acquire));
    assert!(backend.released.load(Ordering::Acquire));
}

#[test]
fn terminal_event_must_match_the_validated_result() {
    let backend = backend();
    backend.result.lock().unwrap().exit_code = Some(1);
    let error = session(backend.clone())
        .run_bounded_script("true", Duration::from_secs(10))
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::WorkerDisconnected);
    assert!(backend.cancelled.load(Ordering::Acquire));
    assert!(backend.released.load(Ordering::Acquire));
}

#[test]
fn cancelled_terminal_event_cannot_report_a_successful_exit_code() {
    let backend = backend();
    let mut events = backend.events.lock().unwrap();
    let terminal = events.last_mut().unwrap();
    terminal.event = ExecutionEventKindV1::Lifecycle {
        state: ExecutionLifecycleV1::Cancelled,
    };
    drop(events);
    {
        let mut result = backend.result.lock().unwrap();
        result.cancelled = true;
        result.failure = Some(crate::execution::errors::ExecutionError::new(
            ExecutionErrorKind::Cancelled,
            "cancelled",
            false,
        ));
    }
    let error = session(backend.clone())
        .run_bounded_script("true", Duration::from_secs(10))
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::WorkerDisconnected);
    assert!(backend.cancelled.load(Ordering::Acquire));
    assert!(backend.released.load(Ordering::Acquire));
}
