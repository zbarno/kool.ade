use super::*;

/// Production boundary: shared durable reply admission and application fencing.
/// Two independent stores and reloads exercise the file lock and retry budget,
/// which in-memory validators cannot prove. Temporary files only; low setup cost.
#[test]
fn recoverable_failure_is_claimed_once_across_windows_and_success_clears_failure() {
    let dir = tempfile::tempdir().unwrap();
    let slug = dir.path().to_str().unwrap();
    let key = "task:answer";
    let mut first = TaskChats::default();
    first.ensure_loaded(slug);
    let user = ChatMessage::new(ChatRole::User, "Choose the first option", None);
    let user_id = user.id.clone();
    let initial = first
        .append_user_reply_and_claim(slug, key, user.clone())
        .unwrap();
    let visible_ts = first.messages[key]
        .iter()
        .find(|message| message.id == initial)
        .unwrap()
        .ts;
    assert!(
        first
            .renew_task_reply_claim(slug, key, &user_id, &initial)
            .unwrap()
    );
    let claim = first.messages[key]
        .iter()
        .find(|message| message.id == initial)
        .unwrap();
    assert_eq!(claim.ts, visible_ts);
    assert!(claim.lease_renewed_at.is_some());
    let mut failure = ChatMessage::new(ChatRole::System, "Malformed JSON", None);
    TaskChats::mark_recoverable_task_reply(&mut failure);
    first.append(slug, key, vec![failure]).unwrap();
    assert!(first.has_pending_user_reply(key));
    assert!(!first.has_active_reply_claim(key));
    let retry = first
        .claim_pending_task_reply(slug, key, &user_id)
        .unwrap()
        .unwrap();
    let mut second = TaskChats::default();
    second.ensure_loaded(slug);
    assert!(
        second
            .claim_pending_task_reply(slug, key, &user_id)
            .unwrap()
            .is_none()
    );
    assert!(
        TaskChats::lock_latest_user_message(slug, key, &user_id, Some(&initial))
            .unwrap()
            .is_none()
    );
    let lock = TaskChats::lock_latest_user_message(slug, key, &user_id, Some(&retry))
        .unwrap()
        .unwrap();
    lock.mark_applied().unwrap();
    drop(lock);
    second.refresh_now(slug);
    assert!(!second.has_pending_user_reply(key));
    assert!(!second.has_failed_task_reply(key));
    assert_eq!(
        second.messages[key]
            .iter()
            .filter(|message| message.role == ChatRole::User)
            .count(),
        1
    );
    assert_eq!(
        second.messages[key]
            .iter()
            .find(|message| message.id == user_id),
        Some(&user)
    );
}

/// Same persistence boundary as the test above; prove the budget survives reload
/// and is not reset by expired claims or another application window.
#[test]
fn repeated_recoverable_failures_stop_without_replaying_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let slug = dir.path().to_str().unwrap();
    let key = "task:answer";
    let mut chats = TaskChats::default();
    chats.ensure_loaded(slug);
    let user = ChatMessage::new(ChatRole::User, "My answer", None);
    let user_id = user.id.clone();
    chats.append_user_reply_and_claim(slug, key, user).unwrap();
    for _ in 0..2 {
        let mut failure = ChatMessage::new(ChatRole::System, "Malformed JSON", None);
        TaskChats::mark_recoverable_task_reply(&mut failure);
        chats.append(slug, key, vec![failure]).unwrap();
    }
    let mut restarted = TaskChats::default();
    restarted.ensure_loaded(slug);
    assert!(!restarted.has_pending_user_reply(key));
    assert!(restarted.has_failed_task_reply(key));
    assert!(
        restarted
            .claim_pending_task_reply(slug, key, &user_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn legacy_format_failures_are_recoverable_but_unknown_provider_and_authorization_failures_are_not()
{
    for (text, safe) in [
        (
            "⚠ Turn rejected — nothing was written.\nStructured JSON block is malformed (invalid JSON); NO changes were saved.",
            true,
        ),
        (
            "⚠ Turn rejected — nothing was written.\n“CLR-001”: cannot update and resolve the same item in one turn",
            true,
        ),
        ("Planning stopped: ProviderRejectedRequest", false),
        ("cancelled", false),
        (
            "⚠ Turn rejected — nothing was written.\nStructured JSON block is malformed\nHuman item requires an explicit answer",
            false,
        ),
    ] {
        let mut message = ChatMessage::new(ChatRole::System, text, None);
        TaskChats::mark_task_reply_outcome(&mut message);
        assert_eq!(
            super::super::safe_recovery::is_recoverable(&message),
            safe,
            "{text}"
        );
        assert_eq!(has_task_reply_outcome(&[message]), !safe, "{text}");
    }
}

/// Same durable admission boundary; legacy histories had no initial claim.
/// An expired recovery must not gain another attempt after process restart.
#[test]
fn legacy_failure_without_initial_claim_cannot_restart_an_expired_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let slug = dir.path().to_str().unwrap();
    let key = "task:legacy";
    let user = ChatMessage::new(ChatRole::User, "Keep the saved choice", None);
    let user_id = user.id.clone();
    let mut failure = ChatMessage::new(
        ChatRole::System,
        "⚠ Turn rejected — nothing was written.\nStructured JSON block is malformed (invalid JSON); NO changes were saved.",
        None,
    );
    TaskChats::mark_task_reply_outcome(&mut failure);
    let mut chats = TaskChats::default();
    chats.ensure_loaded(slug);
    chats.append(slug, key, vec![user, failure]).unwrap();
    let recovery = chats
        .claim_pending_task_reply(slug, key, &user_id)
        .unwrap()
        .unwrap();
    let (mut stored, _) = super::super::super::read_stored(slug).unwrap();
    let claim = stored
        .other_histories
        .get_mut(key)
        .unwrap()
        .iter_mut()
        .find(|message| message.id == recovery)
        .unwrap();
    claim.ts = chrono::Utc::now() - chrono::Duration::minutes(16);
    let path = crate::persistence::project_dir(slug).join("task-conversations.json");
    crate::artifacts::atomic_write(&path, &serde_json::to_string(&stored).unwrap()).unwrap();
    let mut restarted = TaskChats::default();
    restarted.ensure_loaded(slug);
    assert!(
        restarted
            .claim_pending_task_reply(slug, key, &user_id)
            .unwrap()
            .is_none()
    );
    assert!(!restarted.has_pending_user_reply(key));
    assert!(restarted.has_failed_task_reply(key));
}
