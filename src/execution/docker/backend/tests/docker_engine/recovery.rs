use super::*;
use support::{
    Fixture, OPT_IN, backend_with_owner, container_cleanup, container_id, prepare_and_start,
    request, verification_request_with_artifacts, wait_for_stdout_with_task, workspace,
};

#[path = "recovery/cancel_before_collecting.rs"]
mod cancel_before_collecting;
#[path = "recovery/cancellation_after_stage_out.rs"]
mod cancellation_after_stage_out;
#[path = "recovery/completed_result.rs"]
mod completed_result;
#[path = "recovery/persisted_cancellation.rs"]
mod persisted_cancellation;
#[path = "recovery/pre_command_restart.rs"]
mod pre_command_restart;
#[path = "recovery/stage_in_restart.rs"]
mod stage_in_restart;
#[path = "recovery/stage_out_restart.rs"]
mod stage_out_restart;

/// Production boundary: durable-record recovery after app restart, Docker
/// ownership-label verification/removal, and preservation of the authoritative
/// dirty task clone. A unit test cannot prove the replacement worker reconciles
/// a real container without re-running its command while leaving the host clone
/// intact. Requires native Linux Docker and the locked image to be cached or
/// pullable; moderate setup cost. The test uses no SSH or credentials.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restarted_worker_interrupts_without_replaying_or_discarding_dirty_clone() {
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
    let dirty_file = task_clone.join("unfinished-change.txt");
    fs::write(&dirty_file, "preserved uncommitted content\n").unwrap();
    let owner_id = backend.identity.owner_instance_id.clone();
    let request = request(
        workspace.clone(),
        "printf 'container-only change\\n' > unfinished-change.txt; printf 'restart-ready\\n'; sleep 300; printf 'must-not-complete\\n'",
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
    wait_for_stdout_with_task(&backend, &handle, "restart-ready", &running).await;
    // Aborting exec closes the stats reader's stop channel; the reader must not
    // own RuntimeExecution or its task lock while unwinding for recovery.
    running.abort();
    let _ = running.await;
    drop(backend);

    let (restarted, _) = backend_with_owner(&fixture, workspace.clone(), owner_id);
    restarted
        .probe(&crate::execution::contract::ExecutionTarget::local())
        .await
        .expect("worker restart reconciliation should complete");
    assert_eq!(
        fs::read_to_string(&dirty_file).unwrap(),
        "preserved uncommitted content\n",
        "interruption must preserve the task clone's pre-existing uncommitted data"
    );
    let recovered_record =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("interrupted execution record remains durable");
    assert!(recovered_record.command_started);
    assert_eq!(
        recovered_record.state,
        crate::persistence::execution_records::ExecutionRecordStateV1::Interrupted
    );
    let duplicate_attempt = restarted.exec(&handle, &request).await.unwrap_err();
    assert_eq!(
        duplicate_attempt.kind,
        crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
        "an already-started execution must not be replayed after restart"
    );
    let events = restarted
        .events(&handle, EventPageRequestV1::new(0, 256).unwrap())
        .await
        .expect("recovered event journal should remain readable");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                &event.event,
                ExecutionEventKindV1::Stdout { chunk } if chunk.contains("restart-ready")
            ))
            .count(),
        1,
        "recovery must not append a second command's output"
    );
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

/// Models the container state left when a Docker daemon restart has stopped
/// this task's container. It does not restart the host daemon, which could
/// disrupt unrelated workloads. The production boundary is still real Docker
/// inspect/stop/remove and durable recovery. A unit test cannot prove recovery
/// of a daemon-reported stopped container. Requires native Linux Docker and
/// the locked image cached or pullable; moderate setup cost; no SSH or secrets.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restarted_worker_reconciles_a_stopped_container_without_replaying_command() {
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
    let dirty_file = task_clone.join("dirty-before-restart.txt");
    fs::write(&dirty_file, "preserve this task-local change\n").unwrap();
    let owner_id = backend.identity.owner_instance_id.clone();
    let request = request(
        workspace.clone(),
        "printf 'stopped-restart-ready\\n'; sleep 300; printf 'must-not-replay\\n'",
    );
    let _cleanup = container_cleanup(&request);
    let (_lease, handle) = prepare_and_start(&backend, &request).await;
    let id = container_id(&backend, &handle);
    let backend = Arc::new(backend);
    let running_backend = backend.clone();
    let running_request = request.clone();
    let running_handle = handle.clone();
    let running = tokio::spawn(async move {
        running_backend
            .exec(&running_handle, &running_request)
            .await
    });
    wait_for_stdout(&backend, &handle, "stopped-restart-ready").await;
    running.abort();
    let _ = running.await;

    backend
        .docker
        .stop_container(
            &id,
            Some(bollard::query_parameters::StopContainerOptions {
                t: Some(1),
                ..Default::default()
            }),
        )
        .await
        .expect("stop only the owned test container to model daemon restart");
    let stopped = backend
        .docker
        .inspect_container(
            &id,
            Some(
                bollard::query_parameters::InspectContainerOptionsBuilder::default()
                    .size(false)
                    .build(),
            ),
        )
        .await
        .expect("inspect the stopped test container");
    assert_eq!(
        stopped.state.and_then(|state| state.running),
        Some(false),
        "test setup must leave the owned container stopped"
    );
    drop(backend);

    let (restarted, _) = backend_with_owner(&fixture, workspace.clone(), owner_id);
    restarted
        .probe(&crate::execution::contract::ExecutionTarget::local())
        .await
        .expect("replacement worker should reconcile the stopped container");
    assert!(matches!(
        restarted
            .docker
            .inspect_container(
                &id,
                Some(
                    bollard::query_parameters::InspectContainerOptionsBuilder::default()
                        .size(false)
                        .build(),
                ),
            )
            .await,
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            ..
        })
    ));
    let record =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("stopped execution record remains durable");
    assert_eq!(
        record.state,
        crate::persistence::execution_records::ExecutionRecordStateV1::Interrupted
    );
    assert!(record.command_started);
    assert_eq!(
        fs::read_to_string(&dirty_file).unwrap(),
        "preserve this task-local change\n"
    );
    assert_eq!(
        restarted.exec(&handle, &request).await.unwrap_err().kind,
        crate::execution::errors::ExecutionErrorKind::WorkerDisconnected
    );
    let result = restarted
        .collect_artifacts(&handle)
        .await
        .expect("interrupted result should remain available");
    assert_eq!(
        result.failure.map(|failure| failure.kind),
        Some(crate::execution::errors::ExecutionErrorKind::WorkerDisconnected)
    );
}
