use super::*;

#[test]
fn task_details_show_the_latest_persisted_execution_target_and_image() {
    let root = std::env::temp_dir().join(format!(
        "koolade-task-execution-details-{}",
        uuid::Uuid::new_v4()
    ));
    let _home = crate::test_support::KooladeHomeGuard::set(&root);
    let mut app = board_tests::fixture();
    let ticket = ".koolade-packet/planning/tasks/fixture/002-task.md";
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    if let Screen::Connected(project) = &mut app.screen {
        project
            .implementation_states
            .get_mut(ticket)
            .expect("fixture task exists")
            .task_uid = Some(task_id.clone());
    }
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
        cleanup: crate::persistence::execution_records::ExecutionCleanupStateV1::Complete,
    };
    crate::persistence::execution_records::save(&record).unwrap();

    let view = app.task_detail_view(ticket).expect("connected task model");
    let evidence = view.execution_evidence.expect("saved execution is visible");
    assert_eq!(
        evidence.controller_target_id.as_deref(),
        Some(worker_id.as_str())
    );
    assert_eq!(evidence.image_digest, record.image_digest);
    assert_eq!(evidence.state, record.state);
    drop(_home);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn task_details_read_from_one_model_and_emit_typed_commands() {
    let mut app = board_tests::fixture();
    let ticket = ".koolade-packet/planning/tasks/fixture/002-task.md";
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
