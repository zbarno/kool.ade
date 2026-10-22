use super::*;

#[test]
fn waiting_lifecycle_states_persist_and_restore_without_collapsing() {
    use crate::persistence::execution_records::ExecutionRecordStateV1 as RecordState;

    let _home = IsolatedHome::new();
    let workspace_path = std::env::temp_dir().join(format!(
        "koolade-runtime-wait-workspace-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(workspace_path.join(".git")).unwrap();
    let workspace = ValidatedWorkspace::inspect(&workspace_path).unwrap();
    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let execution_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let budget = ResourceBudgetV1 {
        cpu_millis: 1000,
        memory_bytes: 1024 * 1024 * 1024,
        process_count: 16,
        temporary_disk_bytes: 1024 * 1024 * 1024,
        output_bytes: 1024 * 1024,
    };
    let image = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let deadline_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + crate::execution::contract::MAX_EXECUTION_DURATION_MS
        - 60_000;
    let lease_expires_at_unix_ms = deadline_unix_ms.saturating_add(60_000);
    let request = ExecutionRequestV1 {
        schema_version: 1,
        execution_id: execution_id.clone(),
        project_id: project_id.clone(),
        task_id: task_id.clone(),
        target: ExecutionTarget::local(),
        controller_target_id: None,
        private_files_attached: false,
        workspace: WorkspaceRef {
            project_id,
            task_id: task_id.clone(),
            source_sha: "a".repeat(40),
        },
        destination_sha: Some("c".repeat(40)),
        mode: ExecutionModeV1::Implementation,
        image_digest: image.digest,
        authorization_generation: uuid::Uuid::new_v4().hyphenated().to_string(),
        argv: vec!["sh".into(), "-s".into()],
        working_directory_relative: ".".into(),
        stdin: Some("true".into()),
        environment_allowlist: Default::default(),
        resource_budget: budget.clone(),
        deadline_unix_ms,
        allowed_service_ports: Default::default(),
        broker_capability_ids: Default::default(),
        dependency_cache_refs: Vec::new(),
        cancellation_generation: 1,
        request_sequence: 1,
        policy: ExecutionPolicyV1::for_mode(
            ExecutionModeV1::Implementation,
            budget,
            Default::default(),
        )
        .unwrap(),
    };
    let mut execution = RuntimeExecution::new(
        request,
        RuntimeExecutionContext {
            container_id: "container-test".into(),
            container_name: format!("koolade-exec-{execution_id}"),
            workspace: workspace.clone(),
            lease_id: uuid::Uuid::new_v4().to_string(),
            owner_instance_id: uuid::Uuid::new_v4().to_string(),
            policy_version: "docker-sandbox-v1".into(),
            lease_expires_at_unix_ms,
            task_lock: crate::persistence::execution_records::try_task_lock(&task_id)
                .unwrap()
                .unwrap(),
            workspace_snapshot: None,
        },
    );
    for state in [
        ExecutionLifecycleV1::Queued,
        ExecutionLifecycleV1::Preparing,
        ExecutionLifecycleV1::WaitingForDependencyApproval,
    ] {
        execution
            .push(ExecutionEventKindV1::Lifecycle { state })
            .unwrap();
    }
    let waiting_for_approval = crate::persistence::execution_records::load(&task_id, &execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        waiting_for_approval.state,
        RecordState::WaitingForDependencyApproval
    );
    drop(execution);

    let restored = RuntimeExecution::restore(
        &waiting_for_approval,
        workspace.clone(),
        crate::persistence::execution_records::try_task_lock(&task_id)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        restored.lifecycle_state,
        ExecutionLifecycleV1::WaitingForDependencyApproval
    );
    let mut restored = restored;
    restored
        .push(ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Preparing,
        })
        .unwrap();
    restored.container_id.clear();
    let timeout_error = crate::execution::errors::ExecutionError::new(
        crate::execution::errors::ExecutionErrorKind::DeadlineExceeded,
        "runtime readiness deadline elapsed",
        false,
    );
    let returned_error = crate::execution::docker::backend::lifecycle::persist_runtime_wait_error(
        &mut restored,
        timeout_error.clone(),
    )
    .unwrap();
    assert_eq!(returned_error.message, timeout_error.message);
    let waiting_for_runtime = crate::persistence::execution_records::load(&task_id, &execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(waiting_for_runtime.state, RecordState::WaitingForRuntime);
    drop(restored);

    let mut recovered_after_404 = waiting_for_runtime.clone();
    assert!(
        crate::execution::docker::backend::lifecycle::reconcile_missing_container_404(
            &mut recovered_after_404,
            12,
        )
        .unwrap()
    );
    assert_eq!(recovered_after_404.state, RecordState::WaitingForRuntime);
    assert_eq!(
        recovered_after_404.event_cursor,
        waiting_for_runtime.event_cursor
    );
    let still_waiting = crate::persistence::execution_records::load(&task_id, &execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(still_waiting.state, RecordState::WaitingForRuntime);
    assert_eq!(still_waiting.event_cursor, waiting_for_runtime.event_cursor);

    let restored = RuntimeExecution::restore(
        &waiting_for_runtime,
        workspace.clone(),
        crate::persistence::execution_records::try_task_lock(&task_id)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        restored.lifecycle_state,
        ExecutionLifecycleV1::WaitingForRuntime
    );
    drop(restored);

    let mut failed_retry = RuntimeExecution::restore(
        &waiting_for_runtime,
        workspace,
        crate::persistence::execution_records::try_task_lock(&task_id)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    failed_retry
        .push(ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Preparing,
        })
        .unwrap();
    failed_retry
        .push(ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Running,
        })
        .unwrap();
    let registration_error = crate::execution::errors::ExecutionError::new(
        crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
        "execution registry could not be updated",
        true,
    );
    let persisted_error =
        crate::execution::docker::backend::lifecycle::record_start_registration_failure(
            &mut failed_retry,
            registration_error.clone(),
        );
    assert_eq!(persisted_error.message, registration_error.message);
    let failed = crate::persistence::execution_records::load(&task_id, &execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(failed.state, RecordState::Failed);
    assert_eq!(failed.event_cursor, 8);
    assert!(failed.result.unwrap().failure.is_some());
    drop(failed_retry);
    let _ = fs::remove_dir_all(workspace_path);
}
