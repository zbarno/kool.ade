use super::*;
use std::sync::Arc;

#[path = "commands/recipe_retry.rs"]
mod recipe_retry;

#[test]
fn script_session_uses_stdin_collects_validated_streams_and_releases() {
    let backend = backend();
    let session = session(backend.clone());
    let output = session
        .run_bounded_script("printf hello", Duration::from_secs(10))
        .unwrap();
    assert_eq!(output.stdout, "hello\n");
    assert_eq!(output.stderr, "warning\n");
    assert_eq!(output.result.exit_code, Some(0));
    let request = backend.request.lock().unwrap().clone().unwrap();
    assert_eq!(request.argv, ["sh", "-s"]);
    assert_eq!(request.stdin.as_deref(), Some("printf hello"));
    assert!(backend.released.load(Ordering::Acquire));
    assert!(backend.workspace_released.load(Ordering::Acquire));
}

#[test]
fn execution_request_carries_the_authorized_destination_sha() {
    let backend = backend();
    let mut session = session(backend.clone());
    let destination_sha = "c".repeat(40);
    session.destination_sha = Some(destination_sha.clone());
    session
        .run_bounded_script("printf destination-bound", Duration::from_secs(10))
        .unwrap();
    assert_eq!(
        backend
            .request
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .destination_sha
            .as_deref(),
        Some(destination_sha.as_str())
    );
}

#[test]
fn script_session_rejects_oversized_or_unbounded_requests() {
    let backend = backend();
    let session = session(backend.clone());
    assert!(
        session
            .run_bounded_script(
                &"x".repeat(super::super::super::contract::MAX_SCRIPT_BYTES + 1),
                Duration::from_secs(10)
            )
            .is_err()
    );
    assert!(backend.request.lock().unwrap().is_none());
}

#[test]
fn interrupted_execution_preserves_the_known_workspace_snapshot_for_retry() {
    // Production boundary: an authenticated worker's terminal Interrupted event
    // and durable WorkerDisconnected result must not synchronize its partial
    // workspace over the controller snapshot used for a later retry. Existing
    // worker-restart E2E covers the SSH/Docker recovery boundary; this unit test
    // verifies the controller's final apply decision without Docker. Setup cost:
    // none beyond the in-memory fake backend.
    let backend = interrupted_backend();

    let output = session(backend.clone())
        .run_bounded_script("printf partial", Duration::from_secs(10))
        .unwrap();

    assert_eq!(
        output.result.failure.as_ref().map(|failure| failure.kind),
        Some(ExecutionErrorKind::WorkerDisconnected)
    );
    assert!(!backend.workspace_result_applied.load(Ordering::Acquire));
}

#[test]
fn confirmed_worker_loss_retries_once_on_the_replacement_session() {
    let interrupted = interrupted_backend();
    let replacement = backend();
    let replacement_session = Arc::new(session(replacement.clone()));
    let next = replacement_session.clone();
    let session = session(interrupted.clone());
    session
        .set_remote_retry_factory(Arc::new(move || Ok(next.clone())))
        .unwrap();

    let output = session
        .run_bounded_script("printf retry", Duration::from_secs(10))
        .unwrap();

    assert_eq!(output.result.exit_code, Some(0));
    assert!(!interrupted.workspace_result_applied.load(Ordering::Acquire));
    assert_eq!(
        replacement
            .request
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .stdin
            .as_deref(),
        Some("printf retry")
    );
    assert!(session.retry_destination().unwrap().is_some());
}

#[test]
fn explicit_remote_capacity_rejection_finishes_the_claim_attempt() {
    #[derive(Debug)]
    struct AttemptFence {
        started: AtomicBool,
        finished: AtomicBool,
    }

    impl crate::execution::session::AuthorizationGenerationVerifier for AttemptFence {
        fn generation(&self) -> String {
            "00000000-0000-4000-8000-000000000099".into()
        }

        fn verify(&self, generation: &str) -> Result<(), ExecutionError> {
            if generation == self.generation() {
                Ok(())
            } else {
                Err(ExecutionError::new(
                    ExecutionErrorKind::PolicyDenied,
                    "test generation mismatch",
                    false,
                ))
            }
        }

        fn mark_remote_attempt_started(&self) -> Result<(), ExecutionError> {
            self.started.store(true, Ordering::Release);
            Ok(())
        }

        fn finish_remote_attempt(&self) -> Result<(), ExecutionError> {
            self.finished.store(true, Ordering::Release);
            Ok(())
        }
    }

    let backend = backend();
    *backend.start_error.lock().unwrap() = Some(ExecutionError::new(
        ExecutionErrorKind::WorkerIncompatible,
        crate::execution::errors::REMOTE_START_CAPACITY_REJECTION_MESSAGE,
        true,
    ));
    let verifier = Arc::new(AttemptFence {
        started: AtomicBool::new(false),
        finished: AtomicBool::new(false),
    });
    let mut session = session(backend.clone());
    session.target =
        crate::execution::contract::ExecutionTarget::remote("00000000-0000-4000-8000-000000000004");
    session.authorization_generation =
        crate::execution::session::AuthorizationGenerationVerifier::generation(verifier.as_ref());
    session.authorization_verifier = Some(verifier.clone());

    let error = session
        .run_bounded_script("true", Duration::from_secs(10))
        .unwrap_err();

    assert_eq!(error.kind, ExecutionErrorKind::WorkerIncompatible);
    assert!(verifier.started.load(Ordering::Acquire));
    assert!(verifier.finished.load(Ordering::Acquire));
    assert!(backend.workspace_released.load(Ordering::Acquire));
}

fn interrupted_backend() -> Arc<FakeBackend> {
    let backend = backend();
    *backend.events.lock().unwrap() = vec![
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
        event(
            4,
            ExecutionEventKindV1::Lifecycle {
                state: ExecutionLifecycleV1::Interrupted,
            },
        ),
    ];
    *backend.result.lock().unwrap() = ExecutionResultV1 {
        schema_version: 1,
        execution_id: execution_id(),
        exit_code: None,
        signal: None,
        timed_out: false,
        cancelled: false,
        metrics: ResourceMetricsV1 {
            cpu_millis: 0,
            peak_memory_bytes: 0,
            process_count: 0,
            disk_bytes: 0,
        },
        artifacts: Vec::new(),
        failure: Some(ExecutionError::new(
            ExecutionErrorKind::WorkerDisconnected,
            "Worker stopped before execution completed",
            true,
        )),
    };
    backend
}
