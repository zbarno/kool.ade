use super::*;

#[test]
fn activity_history_round_trips_in_its_own_store() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = std::env::temp_dir().join(format!("manager-activity-home-{}", std::process::id()));
    environment.set("KOOLADE_HOME", &home);
    let slug = format!("manager-activity-{}", std::process::id());
    let message = ChatMessage::new(crate::domain::ChatRole::Agent, "Board checked.", None);
    append(&slug, std::slice::from_ref(&message)).unwrap();
    assert_eq!(load(&slug), vec![message]);
    assert!(path(&slug).ends_with("manager-activity.jsonl"));
}

/// Production boundary: the private Manager activity store must retain an
/// unresolved run checkpoint across its rolling history limit. Invariant:
/// restart can replay the exact event batch until an outcome is appended.
/// External dependencies: none. Setup cost: one temporary local state root.
#[test]
fn rolling_retention_keeps_a_pending_manager_checkpoint_until_outcome() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = std::env::temp_dir().join(format!(
        "manager-checkpoint-home-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    environment.set("KOOLADE_HOME", &home);
    let slug = format!("manager-checkpoint-{}", uuid::Uuid::new_v4());
    let checkpoint = crate::domain::ManagerEventCheckpointV1::from_messages(
        &["saved event".into()],
        None,
        None,
        None,
    )
    .unwrap();
    let run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut start = ChatMessage::new(crate::domain::ChatRole::System, "Manager started", None);
    start.manager_run_event = Some(crate::domain::ManagerRunEventV1 {
        run_id: run_id.clone(),
        sequence: 1,
        outcome: crate::domain::ManagerRunOutcomeV1::Started,
        checkpoint: Some(checkpoint.clone()),
        checkpoint_cursor: checkpoint.cursor,
        checkpoint_sha256: checkpoint.sha256.clone(),
        retry_after_unix_ms: None,
    });
    append(&slug, std::slice::from_ref(&start)).unwrap();
    let chatter = (0..320)
        .map(|index| {
            ChatMessage::new(
                crate::domain::ChatRole::System,
                format!("synthetic event {index}"),
                None,
            )
        })
        .collect::<Vec<_>>();
    append(&slug, &chatter).unwrap();
    let loaded = load(&slug);
    assert!(loaded.iter().any(|message| message.id == start.id));
    assert!(loaded.iter().any(|message| {
        message
            .manager_run_event
            .as_ref()
            .is_some_and(|event| event.checkpoint.as_ref() == Some(&checkpoint))
    }));

    let mut completed =
        ChatMessage::new(crate::domain::ChatRole::System, "Manager completed", None);
    completed.manager_run_event = Some(crate::domain::ManagerRunEventV1 {
        run_id,
        sequence: 2,
        outcome: crate::domain::ManagerRunOutcomeV1::Completed,
        checkpoint: None,
        checkpoint_cursor: checkpoint.cursor,
        checkpoint_sha256: checkpoint.sha256,
        retry_after_unix_ms: None,
    });
    append(&slug, &[completed]).unwrap();
    assert!(
        load(&slug).iter().any(|message| message.id == start.id),
        "a retained terminal event keeps its run anchor for restart sequence validation"
    );
}

#[test]
fn malformed_manager_envelope_is_reported_and_never_compacted_away() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = std::env::temp_dir().join(format!("manager-malformed-home-{}", std::process::id()));
    environment.set("KOOLADE_HOME", &home);
    let slug = format!("manager-malformed-{}", uuid::Uuid::new_v4());
    let checkpoint = crate::domain::ManagerEventCheckpointV1::from_messages(
        &["saved event".into()],
        None,
        None,
        None,
    )
    .unwrap();
    let mut message = ChatMessage::new(crate::domain::ChatRole::System, "Started", None);
    message.manager_run_event = Some(crate::domain::ManagerRunEventV1 {
        run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        sequence: 1,
        outcome: crate::domain::ManagerRunOutcomeV1::Started,
        checkpoint: Some(checkpoint.clone()),
        checkpoint_cursor: checkpoint.cursor,
        checkpoint_sha256: checkpoint.sha256,
        retry_after_unix_ms: None,
    });
    let mut malformed = serde_json::to_value(message).unwrap();
    malformed["managerRunEvent"]["checkpoint"]["events"] =
        serde_json::Value::String("a string where an event sequence is required".into());
    let path = path(&slug);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = format!("{}\n", serde_json::to_string(&malformed).unwrap());
    std::fs::write(&path, &original).unwrap();

    let (loaded, diagnostics) = load_with_diagnostics(&slug);
    assert!(loaded.is_empty());
    assert_eq!(diagnostics.dropped_records, 1);
    let next = ChatMessage::new(
        crate::domain::ChatRole::System,
        "Do not overwrite evidence",
        None,
    );
    assert!(append(&slug, &[next]).is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
}

#[test]
fn rolling_retention_keeps_a_queued_checkpoint_until_recovery() {
    let checkpoint = crate::domain::ManagerEventCheckpointV1::from_messages(
        &["queued event".into()],
        None,
        None,
        None,
    )
    .unwrap();
    let mut queued = ChatMessage::new(crate::domain::ChatRole::System, "Queued", None);
    queued.manager_run_event = Some(crate::domain::ManagerRunEventV1 {
        run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        sequence: 1,
        outcome: crate::domain::ManagerRunOutcomeV1::Queued,
        checkpoint: Some(checkpoint.clone()),
        checkpoint_cursor: checkpoint.cursor,
        checkpoint_sha256: checkpoint.sha256,
        retry_after_unix_ms: Some(1),
    });
    let chatter = (0..KEEP_MESSAGES + 1)
        .map(|index| ChatMessage::new(crate::domain::ChatRole::System, format!("{index}"), None))
        .collect::<Vec<_>>();
    let retained = retain_recent_and_pending([vec![queued.clone()], chatter].concat());
    assert!(retained.iter().any(|message| message.id == queued.id));
}

#[test]
fn compaction_preserves_append_order_for_equal_timestamps() {
    let checkpoint = crate::domain::ManagerEventCheckpointV1::from_messages(
        &["ordered event".into()],
        None,
        None,
        None,
    )
    .unwrap();
    let run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let timestamp = chrono::Utc::now();
    let mut started = ChatMessage::new(crate::domain::ChatRole::System, "Started", None);
    started.id = "run-a".into();
    started.ts = timestamp;
    started.manager_run_event = Some(crate::domain::ManagerRunEventV1 {
        run_id: run_id.clone(),
        sequence: 1,
        outcome: crate::domain::ManagerRunOutcomeV1::Started,
        checkpoint: Some(checkpoint.clone()),
        checkpoint_cursor: checkpoint.cursor,
        checkpoint_sha256: checkpoint.sha256.clone(),
        retry_after_unix_ms: None,
    });
    let mut history = vec![started.clone()];
    history.extend((0..KEEP_MESSAGES).map(|index| {
        let mut message = ChatMessage::new(
            crate::domain::ChatRole::System,
            format!("chat {index}"),
            None,
        );
        message.ts = timestamp;
        message
    }));
    let mut retried = ChatMessage::new(crate::domain::ChatRole::System, "Retried", None);
    retried.id = "run-10".into();
    retried.ts = timestamp;
    retried.manager_run_event = Some(crate::domain::ManagerRunEventV1 {
        run_id,
        sequence: 2,
        outcome: crate::domain::ManagerRunOutcomeV1::Retried,
        checkpoint: None,
        checkpoint_cursor: checkpoint.cursor,
        checkpoint_sha256: checkpoint.sha256,
        retry_after_unix_ms: None,
    });
    history.push(retried.clone());
    let retained = retain_recent_and_pending(history);
    let start = retained
        .iter()
        .position(|message| message.id == started.id)
        .unwrap();
    let retry = retained
        .iter()
        .position(|message| message.id == retried.id)
        .unwrap();
    assert!(start < retry);
}

#[test]
fn rejected_compaction_does_not_append_an_event_before_returning_error() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = std::env::temp_dir().join(format!(
        "manager-oversize-home-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    environment.set("KOOLADE_HOME", &home);
    let slug = format!("manager-oversize-{}", uuid::Uuid::new_v4());
    let oversized = ChatMessage::new(
        crate::domain::ChatRole::System,
        "x".repeat(MAX_BYTES as usize + 1),
        None,
    );
    assert!(append(&slug, &[oversized]).is_err());
    assert!(load(&slug).is_empty());
    assert!(!path(&slug).exists());
}
