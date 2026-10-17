use super::*;
use crate::execution::contract::{EventPageRequestV1, ExecutionEventKindV1, ExecutionLifecycleV1};

/// Production boundary: cancellation wins after Docker has reported command
/// exit but before the success path records `Collecting`. Unit tests cannot
/// prove that active cancellation, durable journal ordering, and the resumed
/// exec finalizer agree on one terminal result. Requires native Linux Docker
/// and the locked image cached or pullable; moderate setup cost. No daemon
/// restart, SSH, or credentials.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_before_collecting_keeps_terminal_journal_valid() {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping Docker Engine integration; set {OPT_IN}=1 to opt in");
        return;
    }
    if !cfg!(target_os = "linux") {
        panic!("{OPT_IN}=1 currently requires a native Linux Docker Engine");
    }

    let fixture = Fixture::new();
    let workspace = workspace();
    let (backend, task_clone) = backend_with_owner(
        &fixture,
        workspace.clone(),
        uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let request = request(
        workspace.clone(),
        "printf 'cancel-race-output\\n' > cancel-race.txt && printf 'cancel-race-write-complete\\n'",
    );
    let _cleanup = container_cleanup(&request);
    let (_lease, handle) = prepare_and_start(&backend, &request).await;
    let (finish_paused, resume_finish) =
        crate::execution::docker::backend::lifecycle::pause_before_collecting(
            request.execution_id.clone(),
        );
    let backend = Arc::new(backend);
    let running = tokio::spawn({
        let backend = backend.clone();
        let handle = handle.clone();
        let request = request.clone();
        async move { backend.exec(&handle, &request).await }
    });
    wait_for_stdout(&backend, &handle, "cancel-race-write-complete").await;
    tokio::time::timeout(Duration::from_secs(60), finish_paused)
        .await
        .expect("command did not reach the pre-collecting test boundary")
        .expect("pre-collecting test hook closed before reporting its boundary");

    backend
        .cancel(&handle, request.cancellation_generation)
        .await
        .expect("active cancellation should win before success finalization");
    assert_eq!(
        fs::read_to_string(task_clone.join("cancel-race.txt"))
            .expect("cancelled workspace file should be present"),
        "cancel-race-output\n",
        "cancellation must stage the completed workspace back before finalization resumes"
    );
    resume_finish
        .send(())
        .expect("resume the exec finalizer after cancellation has committed");
    tokio::time::timeout(Duration::from_secs(15), running)
        .await
        .expect("exec finalizer did not stop after observing cancellation")
        .expect("exec task panicked")
        .expect("cancelled exec finalizer should return cleanly");

    let result = backend
        .collect_artifacts(&handle)
        .await
        .expect("cancelled terminal result should remain readable");
    assert!(result.cancelled);
    assert_eq!(
        fs::read_to_string(task_clone.join("cancel-race.txt")).unwrap(),
        "cancel-race-output\n"
    );
    let events = backend
        .events(&handle, EventPageRequestV1::new(0, 256).unwrap())
        .await
        .expect("cancellation journal should remain valid after finalization resumes");
    let terminal_index = events
        .iter()
        .position(|event| {
            matches!(
                event.event,
                ExecutionEventKindV1::Lifecycle {
                    state: ExecutionLifecycleV1::Cancelled
                }
            )
        })
        .expect("journal should contain the cancellation terminal event");
    assert!(
        !events[terminal_index + 1..].iter().any(|event| matches!(
            event.event,
            ExecutionEventKindV1::Lifecycle {
                state: ExecutionLifecycleV1::Collecting
            }
        )),
        "the resumed success path must not append lifecycle events after cancellation"
    );

    backend
        .release(&handle)
        .await
        .expect("cancelled container should remain safely releasable");
    let record =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("cancelled execution record should remain durable");
    assert_eq!(
        record.state,
        crate::persistence::execution_records::ExecutionRecordStateV1::Cancelled
    );
    assert!(record.phase_timings.workspace_stage_out_ms.is_some());
    assert!(record.phase_timings.cleanup_ms.is_some());
}
