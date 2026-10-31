use super::*;

#[test]
fn task_details_show_the_latest_persisted_execution_target_and_image() {
    let root = std::env::temp_dir().join(format!(
        "koolade-task-execution-details-{}",
        uuid::Uuid::new_v4()
    ));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let mut app = board_tests::fixture();
    let ticket = "planning/tasks/fixture/002-task.md";
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let project_slug = if let Screen::Connected(project) = &mut app.screen {
        project
            .implementation_states
            .get_mut(ticket)
            .expect("fixture task exists")
            .task_uid = Some(task_id.clone());
        project.chat_slug.clone()
    } else {
        panic!("fixture project should be connected")
    };
    let worker_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let record = crate::persistence::execution_records::ExecutionRecordV1 {
        schema_version: 1,
        execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        request: None,
        project_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        task_id: task_id.clone(),
        target_id: "local".into(),
        controller_target_id: Some(worker_id.clone()),
        owner_instance_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        container_name: Some("koolade-exec-task-details".into()),
        workspace_lease_id: None,
        workspace_source_sha: "a".repeat(40),
        destination_sha: None,
        workspace_manifest: None,
        container_id: None,
        command_id: None,
        command_started: true,
        result: None,
        image_digest: format!("sha256:{}", "b".repeat(64)),
        policy_version: "docker-policy-v1".into(),
        lease_generation: 1,
        lease_expires_at_unix_ms: Some(2_000),
        attempt: 1,
        state: crate::persistence::execution_records::ExecutionRecordStateV1::Succeeded,
        event_cursor: 0,
        output_bytes: 0,
        started_at_unix_ms: 10,
        updated_at_unix_ms: 20,
        ended_at_unix_ms: Some(20),
        exit_code: None,
        artifacts: Vec::new(),
        docker_usage: crate::persistence::execution_records::DockerResourceUsageV1 {
            cpu_time_ms: Some(4),
            sampled_peak_memory_bytes: Some(2048),
            latest_process_count: Some(2),
            sample_count: 1,
        },
        phase_timings: crate::persistence::execution_records::ExecutionPhaseTimingsV1 {
            image_preparation_ms: Some(12),
            ..Default::default()
        },
        cleanup: crate::persistence::execution_records::ExecutionCleanupStateV1::Complete,
    };
    crate::persistence::execution_records::save(&record).unwrap();

    let mut harness_usage =
        crate::persistence::telemetry::InvocationRecord::new(&project_slug, "synthetic harness");
    harness_usage.task = Some(task_id.clone());
    harness_usage.phase = Some("implementation".into());
    harness_usage.duration_millis = Some(45);
    harness_usage.model_calls = Some(1);
    harness_usage.input_tokens = Some(10);
    harness_usage.output_tokens = Some(5);
    harness_usage.total_tokens = Some(15);
    crate::persistence::telemetry::append(&project_slug, &harness_usage).unwrap();
    let mut unrelated_usage = harness_usage.clone();
    unrelated_usage.task = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    unrelated_usage.total_tokens = Some(9_999);
    crate::persistence::telemetry::append(&project_slug, &unrelated_usage).unwrap();

    let view = app.task_detail_view(ticket).expect("connected task model");
    let evidence = view.execution_evidence.expect("saved execution is visible");
    assert_eq!(
        evidence.controller_target_id.as_deref(),
        Some(worker_id.as_str())
    );
    assert_eq!(evidence.image_digest, record.image_digest);
    assert_eq!(evidence.state, record.state);
    assert_eq!(evidence.docker_usage, record.docker_usage);
    assert_eq!(evidence.phase_timings, record.phase_timings);
    let metrics = view
        .implementation_metrics
        .expect("task-linked harness metrics are visible");
    assert_eq!(metrics.ai_execution_millis, 45);
    assert_eq!(metrics.model_calls, Some(1));
    assert_eq!(metrics.input_tokens, Some(10));
    assert_eq!(metrics.total_tokens, Some(15));
    drop(_home);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn task_details_read_from_one_model_and_emit_typed_commands() {
    let mut app = board_tests::fixture();
    let ticket = "planning/tasks/fixture/002-task.md";
    if let Screen::Connected(project) = &mut app.screen {
        project.task_chats.messages.insert(
            ticket.into(),
            vec![ChatMessage::new(
                ChatRole::Agent,
                "Which review option should we use?",
                Some(ticket.into()),
            )],
        );
        project
            .task_chats
            .drafts
            .insert(ticket.into(), "Keep the existing approach".into());
    }

    let view = app.task_detail_view(ticket).expect("connected task model");
    assert_eq!(
        view.implementation.as_ref().map(|state| state.status),
        Some(ImplementationStatus::AwaitingReview)
    );
    assert_eq!(view.board_column, 2);
    assert!(!view.implementation_active);
    assert_eq!(view.messages.len(), 1);
    assert_eq!(view.draft, "Keep the existing approach");

    app.dispatch_ui_command(crate::ui::ApplicationCommand::TaskDetail(
        crate::ui::task_detail::Command::UpdateDraft {
            ticket: ticket.into(),
            draft: "Use the existing approach and document why.".into(),
        },
    ));
    app.dispatch_ui_command(crate::ui::ApplicationCommand::TaskDetail(
        crate::ui::task_detail::Command::CancelTask {
            ticket: ticket.into(),
        },
    ));

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected");
    };
    assert_eq!(
        project.task_chats.drafts.get(ticket).map(String::as_str),
        Some("Use the existing approach and document why.")
    );
    assert!(!project.queue.running);
    assert!(!project.queue.recovery_paused);
}
