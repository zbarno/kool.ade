use super::*;
use support::{
    Fixture, OPT_IN, backend_with_owner, container_cleanup, prepare_and_start, request,
    wait_for_stdout, workspace,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restarted_worker_interrupts_unobservable_in_flight_exec() {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping Docker Engine integration; set {OPT_IN}=1 to opt in");
        return;
    }
    if !cfg!(target_os = "linux") {
        panic!("{OPT_IN}=1 currently requires a native Linux Docker Engine");
    }

    let fixture = Fixture::new();
    let workspace = workspace();
    let (backend, _) = backend_with_owner(
        &fixture,
        workspace.clone(),
        uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let owner_id = backend.identity.owner_instance_id.clone();
    let request = request(
        workspace.clone(),
        "printf 'restart-ready\\n'; sleep 300; printf 'must-not-complete\\n'",
    );
    let _cleanup = container_cleanup(&request);
    let (_lease, handle) = prepare_and_start(&backend, &request).await;
    let backend = Arc::new(backend);
    let running_backend = backend.clone();
    let running_request = request.clone();
    let running_handle = handle.clone();
    let running = tokio::spawn(async move {
        running_backend
            .exec(&running_handle, &running_request)
            .await
    });
    wait_for_stdout(&backend, &handle, "restart-ready").await;
    running.abort();
    let _ = running.await;
    drop(backend);

    let (restarted, _) = backend_with_owner(&fixture, workspace, owner_id);
    restarted
        .probe(&crate::execution::contract::ExecutionTarget::local())
        .await
        .expect("worker restart reconciliation should complete");
    let events = restarted
        .events(&handle, EventPageRequestV1::new(0, 256).unwrap())
        .await
        .expect("recovered event journal should remain readable");
    assert!(events.iter().any(|event| matches!(
        event.event,
        ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Interrupted
        }
    )));
    let result = restarted
        .collect_artifacts(&handle)
        .await
        .expect("interrupted result should be durable after restart");
    assert_eq!(result.exit_code, None);
    assert_eq!(
        result.failure.map(|failure| failure.kind),
        Some(crate::execution::errors::ExecutionErrorKind::WorkerDisconnected)
    );
}
