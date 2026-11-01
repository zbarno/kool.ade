use super::*;

#[path = "tests/exhausted.rs"]
mod exhausted;

#[test]
fn conflict_retry_is_keyed_to_latest_answer_and_bounded_by_caller() {
    let mut chats = TaskChats::default();
    let key = "task:uid-1";
    let mut first_retry = ChatMessage::new(ChatRole::System, "Retry internally", None);
    TaskChats::mark_planning_conflict_retry(&mut first_retry);
    let mut second_retry = ChatMessage::new(ChatRole::System, "Retry internally", None);
    TaskChats::mark_planning_conflict_retry(&mut second_retry);
    chats.messages.insert(
        key.into(),
        vec![
            ChatMessage::new(ChatRole::User, "Use release branch", None),
            first_retry,
            second_retry,
        ],
    );
    assert_eq!(chats.pending_planning_conflict_retries(key), Some(2));

    chats.messages.get_mut(key).unwrap().push(ChatMessage::new(
        ChatRole::User,
        "Actually use stable branch",
        None,
    ));
    assert_eq!(chats.pending_planning_conflict_retries(key), None);
}

#[test]
fn a_completed_reply_or_unrelated_notice_is_not_a_conflict_retry() {
    let mut chats = TaskChats::default();
    let key = "task:uid-1";
    let mut marker = ChatMessage::new(ChatRole::System, "Retry internally", None);
    TaskChats::mark_planning_conflict_retry(&mut marker);
    chats.messages.insert(
        key.into(),
        vec![
            ChatMessage::new(ChatRole::User, "Use release branch", None),
            marker.clone(),
            ChatMessage::new(ChatRole::Agent, "Done", None),
        ],
    );
    assert_eq!(chats.pending_planning_conflict_retries(key), None);
    chats.messages.insert(
        key.into(),
        vec![
            ChatMessage::new(ChatRole::User, "Use release branch", None),
            marker,
            ChatMessage::new(ChatRole::System, "An unrelated notice", None),
        ],
    );
    assert_eq!(chats.pending_planning_conflict_retries(key), None);
}

#[test]
fn clarification_only_agent_reply_does_not_hide_a_saved_retry() {
    let key = "task:uid-clarification";
    let user = ChatMessage::new(ChatRole::User, "Use release branch", Some(key.into()));
    let mut retry = ChatMessage::new(ChatRole::System, "Retry internally", None);
    TaskChats::mark_planning_conflict_retry(&mut retry);
    let mut clarification = ChatMessage::new(ChatRole::User, "What does release mean?", None);
    clarification.clarification = true;
    let mut response = ChatMessage::new(ChatRole::Agent, "It means the stable branch.", None);
    response.clarification = true;
    let mut claim = ChatMessage::new(ChatRole::System, "Retry claimed", Some(key.into()));
    claim.id = format!("{}{}", claim_scope(&user.id, 1), uuid::Uuid::new_v4());

    let mut chats = TaskChats::default();
    chats.messages.insert(
        key.into(),
        vec![user, retry, clarification, response, claim.clone()],
    );
    assert!(has_active_retry_claim(&chats.messages[key]));
    assert!(chats.has_pending_user_reply(key));

    chats.messages.get_mut(key).unwrap().last_mut().unwrap().ts =
        chrono::Utc::now() - chrono::Duration::minutes(16);
    assert!(!has_active_retry_claim(&chats.messages[key]));
    assert_eq!(chats.pending_planning_conflict_retries(key), Some(1));
}

/// Production boundary: durable retry claims shared by independent windows. This proves only one window can claim a reply and that a claimed reply blocks a competing draft; an in-memory test cannot cover the persisted claim lock. Uses a temporary local filesystem, no external services, low setup cost.
#[test]
fn only_one_window_claims_a_reply_and_active_retry_preserves_a_new_draft() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_retry_claim_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let slug = dir.to_str().unwrap();
    let key = "task:uid-1";
    let mut first = TaskChats::default();
    first.ensure_loaded(slug);
    let user = ChatMessage::new(ChatRole::User, "Use release branch", Some(key.into()));
    let user_id = user.id.clone();
    let mut marker = ChatMessage::new(ChatRole::System, "Retry internally", None);
    TaskChats::mark_planning_conflict_retry(&mut marker);
    first.append(slug, key, vec![user, marker]).unwrap();
    assert_eq!(first.pending_planning_conflict_retries(key), Some(1));

    let mut second_window = TaskChats::default();
    second_window.ensure_loaded(slug);
    assert!(
        first
            .claim_pending_planning_conflict_retry(slug, key, &user_id, 1)
            .unwrap()
            .is_some()
    );
    assert!(
        second_window
            .claim_pending_planning_conflict_retry(slug, key, &user_id, 1)
            .unwrap()
            .is_none()
    );

    assert!(
        second_window
            .append(
                slug,
                key,
                vec![ChatMessage::new(
                    ChatRole::User,
                    "Use the stable branch instead",
                    Some(key.into()),
                )],
            )
            .is_err()
    );
    assert!(
        first
            .claim_pending_planning_conflict_retry(slug, key, &user_id, 1)
            .unwrap()
            .is_none()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// Production boundary: final task-reply persistence lock. This proves another window cannot save inside the guarded apply and an older answer is rejected afterward; an in-memory test cannot exercise the cross-window file lock. Uses a temporary local filesystem, no external services, low setup cost.
#[test]
fn final_reply_lock_serializes_and_rejects_an_old_answer() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_reply_lock_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let slug = dir.to_str().unwrap();
    let key = "task:uid-1";
    let mut first = TaskChats::default();
    first.ensure_loaded(slug);
    let original = ChatMessage::new(ChatRole::User, "Use release branch", Some(key.into()));
    let original_id = original.id.clone();
    first.append(slug, key, vec![original]).unwrap();

    let lock = TaskChats::lock_latest_user_message(slug, key, &original_id, None)
        .unwrap()
        .expect("the original answer is current");
    let mut second_window = TaskChats::default();
    assert!(
        second_window
            .append(
                slug,
                key,
                vec![ChatMessage::new(
                    ChatRole::User,
                    "Use stable branch instead",
                    Some(key.into()),
                )],
            )
            .is_err(),
        "a peer cannot save a correction inside the final write boundary"
    );
    drop(lock);
    second_window
        .append(
            slug,
            key,
            vec![ChatMessage::new(
                ChatRole::User,
                "Use stable branch instead",
                Some(key.into()),
            )],
        )
        .unwrap();
    assert!(
        TaskChats::lock_latest_user_message(slug, key, &original_id, None)
            .unwrap()
            .is_none(),
        "a turn for an older answer cannot cross its write boundary"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// Production boundary: persisted retry-claim takeover after lease expiry. This proves a stale owner cannot renew or apply after another window takes over; an in-memory test cannot cover durable owner fencing across reload. Uses a temporary local filesystem, no external services, low setup cost.
#[test]
fn retry_claim_takeover_fences_old_owner_renewal_and_final_write() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_retry_claim_fence_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let slug = dir.to_str().unwrap();
    let key = "task:uid-1";
    let mut first_window = TaskChats::default();
    first_window.ensure_loaded(slug);
    let user = ChatMessage::new(ChatRole::User, "Use release branch", Some(key.into()));
    let user_id = user.id.clone();
    let mut marker = ChatMessage::new(ChatRole::System, "Retry internally", None);
    TaskChats::mark_planning_conflict_retry(&mut marker);
    first_window.append(slug, key, vec![user, marker]).unwrap();
    let first_claim = first_window
        .claim_pending_planning_conflict_retry(slug, key, &user_id, 1)
        .unwrap()
        .expect("first window claims the retry");

    let (mut stored, _) = super::super::read_stored(slug).unwrap();
    let old_claim = stored
        .other_histories
        .get_mut(key)
        .unwrap()
        .iter_mut()
        .find(|message| message.id == first_claim)
        .unwrap();
    old_claim.ts = chrono::Utc::now() - chrono::Duration::minutes(16);
    let json = serde_json::to_string(&stored).unwrap();
    let path = crate::persistence::project_dir(slug).join("task-conversations.json");
    crate::artifacts::atomic_write(&path, &json).unwrap();
    assert!(
        TaskChats::lock_latest_user_message(slug, key, &user_id, Some(&first_claim))
            .unwrap()
            .is_none()
    );

    let mut second_window = TaskChats::default();
    second_window.ensure_loaded(slug);
    let second_claim = second_window
        .claim_pending_planning_conflict_retry(slug, key, &user_id, 1)
        .unwrap()
        .expect("stale lease allows exactly one recovery takeover");
    assert_ne!(second_claim, first_claim);
    assert!(
        !first_window
            .renew_task_reply_claim(slug, key, &user_id, &first_claim)
            .unwrap()
    );
    assert!(
        TaskChats::lock_latest_user_message(slug, key, &user_id, Some(&first_claim))
            .unwrap()
            .is_none()
    );
    assert!(
        TaskChats::lock_latest_user_message(slug, key, &user_id, None)
            .unwrap()
            .is_none()
    );
    let current = TaskChats::lock_latest_user_message(slug, key, &user_id, Some(&second_claim))
        .unwrap()
        .expect("only the current claim token crosses the write boundary");
    current.mark_applied().unwrap();
    drop(current);
    second_window.refresh_now(slug);
    assert_eq!(second_window.pending_planning_conflict_retries(key), None);
    std::fs::remove_dir_all(dir).unwrap();
}
