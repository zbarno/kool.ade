use super::*;

/// Production boundary: a replacement worker restores an owned running Docker
/// container whose durable record says the command has not started. Unit tests
/// cannot prove the real daemon container remains usable across app shutdown or
/// that the command is launched exactly once after recovery. Requires native
/// Linux Docker and the locked image cached or pullable; moderate setup cost.
/// No SSH, credentials, or host daemon restart are involved.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_before_command_start_restores_container_and_runs_command_once() {
    run_restart_before_command(false).await;
}

/// Production boundary: resuming a v1 Running execution created by the
/// pre-telemetry writer, which persisted Running after a complete upload but
/// has no stage-in timing fields. Unit tests cannot prove the existing daemon
/// container remains usable and starts the command exactly once. Requires
/// native Linux Docker and the locked image cached or pullable; moderate setup
/// cost. No SSH, credentials, or host daemon restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_running_record_without_stage_in_timing_still_resumes() {
    run_restart_before_command(true).await;
}

async fn run_restart_before_command(legacy_record: bool) {
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
    fs::write(task_clone.join("dirty-before-command.txt"), "keep me\n").unwrap();
    let owner_id = backend.identity.owner_instance_id.clone();
    let request = request(
        workspace.clone(),
        "printf 'once\\n' >> command-count.txt; printf 'post-restart\\n'",
    );
    let _cleanup = container_cleanup(&request);
    let (_lease, handle) = prepare_and_start(&backend, &request).await;
    let mut before =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("prepared execution record is durable");
    if legacy_record {
        // The original v1 writer emitted Running only after stage-in completed
        // and did not write phase timings. Its absent timing object deserializes
        // to this default value.
        before.phase_timings =
            crate::persistence::execution_records::ExecutionPhaseTimingsV1::default();
        crate::persistence::execution_records::save(&before).unwrap();
    }
    assert!(!before.command_started);
    assert_eq!(
        before.state,
        crate::persistence::execution_records::ExecutionRecordStateV1::Running
    );
    if legacy_record {
        assert_eq!(
            before.phase_timings,
            crate::persistence::execution_records::ExecutionPhaseTimingsV1::default()
        );
    } else {
        assert!(before.phase_timings.image_preparation_ms.is_some());
        assert!(before.phase_timings.workspace_stage_in_ms.is_some());
        assert!(before.phase_timings.dependency_cache_stage_ms.is_some());
    }
    drop(backend);

    let (restarted, _) = backend_with_owner(&fixture, workspace.clone(), owner_id);
    restarted
        .probe(&crate::execution::contract::ExecutionTarget::local())
        .await
        .expect("replacement worker should restore pre-command container");
    restarted
        .exec(&handle, &request)
        .await
        .expect("restored container should execute the command");
    let result = restarted
        .collect_artifacts(&handle)
        .await
        .expect("completed command result should be available");
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(
        fs::read_to_string(task_clone.join("command-count.txt")).unwrap(),
        "once\n"
    );
    assert_eq!(
        fs::read_to_string(task_clone.join("dirty-before-command.txt")).unwrap(),
        "keep me\n"
    );
    let after =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("completed record remains linked to this task");
    assert_eq!(
        after.state,
        crate::persistence::execution_records::ExecutionRecordStateV1::Succeeded
    );
    assert!(after.command_started);
    if legacy_record {
        assert_eq!(
            after.phase_timings.image_preparation_ms, None,
            "recovery cannot reconstruct legacy image preparation timing"
        );
        assert_eq!(after.phase_timings.workspace_stage_in_ms, None);
        assert_eq!(after.phase_timings.dependency_cache_stage_ms, None);
        assert!(
            after.phase_timings.workspace_stage_out_ms.is_some(),
            "the resumed execution should record its new stage-out timing"
        );
    } else {
        assert!(after.phase_timings.image_preparation_ms.is_some());
        assert!(after.phase_timings.workspace_stage_in_ms.is_some());
        assert!(after.phase_timings.dependency_cache_stage_ms.is_some());
    }
}
