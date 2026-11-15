use super::*;

#[test]
fn manager_checkpoint_is_bounded_and_rejects_string_sequence_drift() {
    let checkpoint = ManagerEventCheckpointV1::from_messages(
        &["first event".into(), "second event".into()],
        None,
        None,
        None,
    )
    .unwrap();
    checkpoint.validate().unwrap();
    assert_eq!(checkpoint.cursor, 2);
    assert_ne!(checkpoint.events[0].id, checkpoint.events[1].id);

    let malformed = serde_json::json!({
        "cursor": 1,
        "events": "a string where an event sequence is required",
        "sha256": "0".repeat(64)
    });
    assert!(serde_json::from_value::<ManagerEventCheckpointV1>(malformed).is_err());
}

#[test]
fn first_queued_or_started_event_requires_its_checkpoint() {
    let event = ManagerRunEventV1 {
        run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        sequence: 1,
        outcome: ManagerRunOutcomeV1::Started,
        checkpoint: None,
        checkpoint_cursor: 1,
        checkpoint_sha256: "a".repeat(64),
        retry_after_unix_ms: None,
    };
    assert!(event.validate().is_err());
}

#[test]
fn manager_event_sequence_rejects_gaps_and_reordering() {
    let checkpoint =
        ManagerEventCheckpointV1::from_messages(&["event".into()], None, None, None).unwrap();
    let first = ManagerRunEventV1 {
        run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        sequence: 1,
        outcome: ManagerRunOutcomeV1::Started,
        checkpoint_cursor: checkpoint.cursor,
        checkpoint_sha256: checkpoint.sha256.clone(),
        checkpoint: Some(checkpoint),
        retry_after_unix_ms: None,
    };
    first.validate_after(None).unwrap();
    let completed = ManagerRunEventV1 {
        run_id: first.run_id.clone(),
        sequence: 3,
        outcome: ManagerRunOutcomeV1::Completed,
        checkpoint: None,
        checkpoint_cursor: first.checkpoint_cursor,
        checkpoint_sha256: first.checkpoint_sha256.clone(),
        retry_after_unix_ms: None,
    };
    assert!(completed.validate_after(Some(&first)).is_err());
}

#[test]
fn checkpoint_retry_budget_resets_after_terminal_completion() {
    let digest = "a".repeat(64);
    let history = [
        ManagerRunOutcomeV1::Started,
        ManagerRunOutcomeV1::Failed,
        ManagerRunOutcomeV1::Retried,
        ManagerRunOutcomeV1::Started,
        ManagerRunOutcomeV1::Completed,
        ManagerRunOutcomeV1::Started,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, outcome)| {
        let mut message =
            super::super::ChatMessage::new(super::super::ChatRole::System, "event", None);
        message.manager_run_event = Some(ManagerRunEventV1 {
            run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            sequence: index as u64 + 1,
            outcome,
            checkpoint: None,
            checkpoint_cursor: 1,
            checkpoint_sha256: digest.clone(),
            retry_after_unix_ms: None,
        });
        message
    })
    .collect::<Vec<_>>();
    assert_eq!(checkpoint_attempts(&history, &digest), 1);
}
