use super::{CancellationIntentGuard, is_worker_loss_result, worker_loss_is_reconciled};
use crate::execution::{
    contract::{ExecutionResultV1, ResourceMetricsV1},
    errors::{ExecutionError, ExecutionErrorKind},
};

fn result(failure: ExecutionErrorKind, retryable: bool) -> ExecutionResultV1 {
    ExecutionResultV1 {
        schema_version: 1,
        execution_id: "00000000-0000-4000-8000-000000000003".into(),
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
        phase_timings: Default::default(),
        artifacts: Vec::new(),
        failure: Some(ExecutionError::new(failure, "fixture", retryable)),
    }
}

#[test]
fn only_retryable_worker_disconnect_results_can_enter_loss_confirmation() {
    assert!(is_worker_loss_result(&result(
        ExecutionErrorKind::WorkerDisconnected,
        true
    )));
    assert!(!is_worker_loss_result(&result(
        ExecutionErrorKind::WorkerDisconnected,
        false
    )));
    assert!(!is_worker_loss_result(&result(
        ExecutionErrorKind::ExecutionFailed,
        true
    )));
}

#[test]
fn worker_loss_requires_a_terminal_interrupted_record_and_completed_container_cleanup() {
    use crate::persistence::execution_records::{
        ExecutionCleanupStateV1 as Cleanup, ExecutionRecordStateV1 as State,
    };

    assert!(worker_loss_is_reconciled(
        State::Interrupted,
        Cleanup::Complete
    ));
    assert!(!worker_loss_is_reconciled(
        State::Interrupted,
        Cleanup::Pending
    ));
    assert!(!worker_loss_is_reconciled(
        State::Interrupted,
        Cleanup::Failed
    ));
    assert!(!worker_loss_is_reconciled(
        State::Running,
        Cleanup::Complete
    ));
}

#[tokio::test]
async fn aborted_cancellation_releases_its_terminal_result_reservation() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let requests = Arc::new(AtomicUsize::new(0));
    let task_requests = Arc::clone(&requests);
    let task = tokio::spawn(async move {
        let _intent = CancellationIntentGuard::new(task_requests);
        std::future::pending::<()>().await;
    });
    tokio::task::yield_now().await;
    assert_eq!(requests.load(Ordering::Acquire), 1);

    task.abort();
    let _ = task.await;

    assert_eq!(requests.load(Ordering::Acquire), 0);
}
