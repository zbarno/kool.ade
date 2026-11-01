use super::*;

/// Production boundary: task-chat persistence and immutable identity rebinding. This proves a reused display key writes only to its new UID stream after reload; an in-memory unit test cannot cover persisted history migration. Uses a temporary local filesystem, no external services, low setup cost.
#[test]
fn reused_board_keys_keep_histories_separated_by_immutable_identity() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_task_identity_reuse_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let slug = dir.to_str().unwrap();
    let mut chats = TaskChats::default();
    chats
        .bind_conversation_identities(&[("feature:F1".into(), "old-feature-uid".into())], &[])
        .unwrap();
    chats.ensure_loaded(slug);
    chats
        .append(
            slug,
            "feature:F1",
            vec![ChatMessage::new(
                ChatRole::User,
                "Old feature conversation",
                None,
            )],
        )
        .unwrap();

    chats
        .bind_conversation_identities(&[("feature:F1".into(), "new-feature-uid".into())], &[])
        .unwrap();
    assert!(!chats.messages.contains_key("feature:F1"));
    assert!(
        chats.messages["@unlinked:old-feature-uid"]
            .iter()
            .any(|message| message.text == "Old feature conversation")
    );
    chats
        .append(
            slug,
            "feature:F1",
            vec![ChatMessage::new(
                ChatRole::User,
                "New feature conversation",
                None,
            )],
        )
        .unwrap();
    assert!(
        chats.messages["feature:F1"]
            .iter()
            .all(|message| message.text != "Old feature conversation")
    );
    assert!(
        chats.messages["feature:F1"]
            .iter()
            .any(|message| message.text == "New feature conversation")
    );
    let latest = ChatMessage::new(ChatRole::User, "Update the current feature", None);
    let latest_id = latest.id.clone();
    chats.append(slug, "feature:F1", vec![latest]).unwrap();
    let (stored, _) = super::super::storage::read_stored(slug).unwrap();
    assert_eq!(stored.task_identities["feature:F1"], "new-feature-uid");
    assert!(
        TaskChats::lock_latest_user_message(slug, "feature:F1", &latest_id, None)
            .unwrap()
            .is_some()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// Production boundary: loading legacy persisted task-chat history against current feature identity. This proves stale history is detached before it can enter the current conversation; an in-memory unit test cannot cover storage reload and migration together. Uses a temporary local filesystem, no external services, low setup cost.
#[test]
fn legacy_display_key_history_is_detached_from_a_reused_feature_id() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_task_legacy_identity_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let slug = dir.to_str().unwrap();
    let mut legacy = TaskChats::default();
    legacy
        .append(
            slug,
            "feature:F1",
            vec![ChatMessage::new(
                ChatRole::User,
                "History from the prior F1 feature",
                None,
            )],
        )
        .unwrap();

    let mut current = TaskChats::default();
    current
        .bind_conversation_identities(&[("feature:F1".into(), "current-feature-uid".into())], &[])
        .unwrap();
    current.ensure_loaded(slug);
    assert!(!current.messages.contains_key("feature:F1"));
    assert!(
        current.messages["@legacy:feature:F1"]
            .iter()
            .any(|message| message.text == "History from the prior F1 feature")
    );
    assert!(
        !current
            .project_context("", 24000)
            .contains("History from the prior F1 feature")
    );

    current
        .append(
            slug,
            "feature:F1",
            vec![ChatMessage::new(
                ChatRole::User,
                "History from the current F1 feature",
                None,
            )],
        )
        .unwrap();
    assert!(
        current.messages["feature:F1"]
            .iter()
            .all(|message| message.text != "History from the prior F1 feature")
    );
    assert!(
        current.messages["feature:F1"]
            .iter()
            .any(|message| message.text == "History from the current F1 feature")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// Production boundary: task-chat identity resolution over persisted histories. This proves ambiguous aliases fail closed without merging either history; a pure validator test cannot prove existing stored streams remain isolated after reload. Uses a temporary local filesystem, no external services, low setup cost.
#[test]
fn conflicting_current_aliases_leave_both_conversations_unlinked() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_task_alias_conflict_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let slug = dir.to_str().unwrap();
    let mut original = TaskChats::default();
    original
        .bind_conversation_identities(
            &[
                ("old-open-key".into(), "open-item-uid".into()),
                ("old-resolved-key".into(), "resolved-item-uid".into()),
            ],
            &[],
        )
        .unwrap();
    original.ensure_loaded(slug);
    original
        .append(
            slug,
            "old-open-key",
            vec![ChatMessage::new(ChatRole::User, "Open item history", None)],
        )
        .unwrap();
    original
        .append(
            slug,
            "old-resolved-key",
            vec![ChatMessage::new(
                ChatRole::User,
                "Resolved item history",
                None,
            )],
        )
        .unwrap();

    let mut current = TaskChats::default();
    let error = current
        .bind_conversation_identities(
            &[
                ("CLR-001".into(), "open-item-uid".into()),
                ("CLR-001".into(), "resolved-item-uid".into()),
            ],
            &[],
        )
        .unwrap_err();
    assert!(error.contains("refers to multiple immutable identities"));
    current.ensure_loaded(slug);
    assert!(!current.messages.contains_key("CLR-001"));
    assert!(
        current.messages["@unlinked:open-item-uid"]
            .iter()
            .any(|message| message.text == "Open item history")
    );
    assert!(
        current.messages["@unlinked:resolved-item-uid"]
            .iter()
            .any(|message| message.text == "Resolved item history")
    );
    std::fs::remove_dir_all(dir).unwrap();
}
