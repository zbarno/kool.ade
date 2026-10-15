use std::{
    fs,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use super::support::{
    Fixture, OPT_IN, backend, container_cleanup, now_unix_ms, prepare_and_start, request,
    wait_for_stdout, workspace,
};
use crate::execution::{backend::ExecutionBackend, contract::ExecutionLifecycleV1};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deadline_preserves_workspace_before_container_teardown() {
    run(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_waiting_for_sync_owns_the_result_after_the_command_deadline() {
    run(true).await;
}

async fn run(cancel_at_deadline: bool) {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping Docker deadline integration; set {OPT_IN}=1 to opt in");
        return;
    }
    let fixture = Fixture::new();
    let workspace = workspace();
    let (backend, clone) = backend(&fixture, workspace.clone());
    let mut request = request(
        workspace,
        "printf 'preserved\\n' > output.txt; printf 'ready\\n'; sleep 300",
    );
    // Exercise the actual deadline and cancellation race without a 30-second wait.
    // Leave ample room for rootless Docker startup under shared CI load.
    request.deadline_unix_ms = now_unix_ms() + 15_000;
    let _cleanup = container_cleanup(&request);
    let (_lease, handle) = prepare_and_start(&backend, &request).await;
    let backend = Arc::new(backend);
    let running = tokio::spawn({
        let backend = backend.clone();
        let handle = handle.clone();
        let request = request.clone();
        async move { backend.exec(&handle, &request).await }
    });
    wait_for_stdout(&backend, &handle, "ready").await;
    if cancel_at_deadline {
        let state = backend
            .executions
            .lock()
            .unwrap()
            .get(&handle.execution_id)
            .unwrap()
            .clone();
        let workspace_sync = state.lock().unwrap().workspace_sync.clone();
        let held_sync = workspace_sync.lock().await;
        let cancelling = tokio::spawn({
            let backend = backend.clone();
            let handle = handle.clone();
            let generation = request.cancellation_generation;
            async move { backend.cancel(&handle, generation).await }
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            while state
                .lock()
                .unwrap()
                .cancellation_requests
                .load(Ordering::Acquire)
                == 0
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("cancellation must reserve the terminal result");
        let remaining = request.deadline_unix_ms.saturating_sub(now_unix_ms());
        tokio::time::sleep(Duration::from_millis(remaining + 150)).await;
        drop(held_sync);
        tokio::time::timeout(Duration::from_secs(15), cancelling)
            .await
            .expect("cancellation remained blocked after synchronization")
            .unwrap()
            .expect("cancel the owned container");
    }
    tokio::time::timeout(Duration::from_secs(40), running)
        .await
        .expect("command did not terminate within its deadline")
        .unwrap()
        .expect("bounded command cleanup");
    let result = backend.collect_artifacts(&handle).await.unwrap();
    assert_eq!(result.cancelled, cancel_at_deadline, "{result:?}");
    assert_eq!(result.timed_out, !cancel_at_deadline, "{result:?}");
    assert_eq!(
        fs::read_to_string(clone.join("output.txt")).unwrap(),
        "preserved\n"
    );
    let terminal_events = backend
        .events(
            &handle,
            crate::execution::contract::EventPageRequestV1::new(0, 256).unwrap(),
        )
        .await
        .unwrap();
    let terminal = terminal_events
        .iter()
        .filter(|event| {
            matches!(
                event.event,
                crate::execution::contract::ExecutionEventKindV1::Lifecycle {
                    state: ExecutionLifecycleV1::Succeeded
                        | ExecutionLifecycleV1::Failed
                        | ExecutionLifecycleV1::Cancelled
                        | ExecutionLifecycleV1::Interrupted
                }
            )
        })
        .count();
    assert_eq!(terminal, 1, "one terminal event must be published");
    backend.release(&handle).await.unwrap();
}
