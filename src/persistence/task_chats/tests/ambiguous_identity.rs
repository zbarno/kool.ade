use super::*;

/// Production boundary: loading legacy task-chat history against ambiguous current aliases. This proves old answers are detached and not written under either UID; an in-memory unit test cannot cover storage reload and migration together. Uses a temporary local filesystem, no external services, low setup cost.
#[test]
fn ambiguous_display_key_detaches_legacy_history_and_cannot_write_to_old_uid() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_task_ambiguous_legacy_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let slug = dir.to_str().unwrap();
    let mut legacy = TaskChats::default();
    legacy
        .append(
            slug,
            "CLR-001",
            vec![ChatMessage::new(
                ChatRole::User,
                "Unattributed legacy answer",
                None,
            )],
        )
        .unwrap();

    let mut current = TaskChats::default();
    current
        .bind_conversation_identities(
            &[
                ("CLR-001".into(), "open-item-uid".into()),
                ("CLR-001".into(), "resolved-item-uid".into()),
            ],
            &[],
        )
        .unwrap_err();
    current.ensure_loaded(slug);
    assert!(current.has_ambiguous_identity("CLR-001"));
    assert!(!current.messages.contains_key("CLR-001"));
    assert!(current.messages.contains_key("@legacy:CLR-001"));
    assert!(
        !current
            .project_context("", 24000)
            .contains("Unattributed legacy answer")
    );

    current
        .append(
            slug,
            "CLR-001",
            vec![ChatMessage::new(
                ChatRole::User,
                "Reply while identity is ambiguous",
                None,
            )],
        )
        .unwrap();
    assert!(current.messages.contains_key("@ambiguous:CLR-001"));
    assert!(
        !current
            .project_context("", 24000)
            .contains("Reply while identity is ambiguous")
    );

    let (stored, _) = super::super::storage::read_stored(slug).unwrap();
    assert!(!stored.task_identities.contains_key("CLR-001"));
    assert!(stored.other_histories.contains_key("@legacy:CLR-001"));
    assert!(stored.other_histories.contains_key("@ambiguous:CLR-001"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn every_path_sharing_a_duplicated_uid_is_marked_ambiguous() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_task_duplicate_uid_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let slug = dir.to_str().unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let mut chats = TaskChats::default();
    chats
        .bind_conversation_identities(
            &[
                ("feature:F1".into(), "shared-uid".into()),
                ("feature:F2".into(), "shared-uid".into()),
                ("feature:F3".into(), "shared-uid".into()),
            ],
            &[],
        )
        .unwrap_err();
    chats.ensure_loaded(slug);
    for key in ["feature:F1", "feature:F2", "feature:F3"] {
        assert!(chats.has_ambiguous_identity(key), "{key} must be fenced");
        assert!(!chats.messages.contains_key(key));
    }
    std::fs::remove_dir_all(dir).unwrap();
}
