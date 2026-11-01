use super::super::*;

#[test]
/// Production boundary: the retry coordinator records exhaustion under the
/// same durable conversation lock, so restart does not replay or re-ask for
/// the answer. Uses temporary local files only.
fn exhausted_conflict_reply_is_terminal_and_kept_for_internal_recovery() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_retry_exhausted_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let slug = dir.to_str().unwrap();
    let key = "task:uid-exhausted";
    let user = ChatMessage::new(ChatRole::User, "Keep the current release branch", None);
    let user_id = user.id.clone();
    let mut first_conflict = ChatMessage::new(ChatRole::System, "Planning changed", None);
    TaskChats::mark_planning_conflict_retry(&mut first_conflict);
    let mut chats = TaskChats::default();
    chats.ensure_loaded(slug);
    chats.append(slug, key, vec![user, first_conflict]).unwrap();
    let first_claim = chats
        .claim_pending_planning_conflict_retry(slug, key, &user_id, 1)
        .unwrap()
        .expect("one bounded internal retry can be claimed");
    assert!(!first_claim.is_empty());

    let mut second_conflict = ChatMessage::new(ChatRole::System, "Planning changed again", None);
    TaskChats::mark_planning_conflict_retry(&mut second_conflict);
    chats.append(slug, key, vec![second_conflict]).unwrap();
    assert_eq!(chats.pending_planning_conflict_retries(key), Some(2));
    assert!(
        chats
            .mark_planning_conflict_retry_exhausted(slug, key, &user_id, 2)
            .unwrap()
    );

    assert_eq!(chats.pending_planning_conflict_retries(key), None);
    assert!(!chats.has_pending_user_reply(key));
    assert!(chats.has_failed_task_reply(key));
    let marker = chats.messages[key]
        .iter()
        .find(|message| {
            message
                .id
                .starts_with("internal-task-reply-outcome:exhausted:")
        })
        .unwrap();
    assert!(marker.text.contains("no repeat answer is needed"));

    let mut reopened = TaskChats::default();
    reopened.ensure_loaded(slug);
    assert!(!reopened.has_pending_user_reply(key));
    assert!(reopened.has_failed_task_reply(key));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
/// Production boundary: a reply whose final worker lease expired cannot
/// exceed its takeover budget, so its saved answer is terminalized durably.
/// Uses temporary conversation files and synthetic expired leases only.
fn second_crashed_retry_takeover_becomes_terminal_internal_recovery() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_retry_claims_exhausted_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let slug = dir.to_str().unwrap();
    let key = "task:uid-claim-exhausted";
    let user = ChatMessage::new(ChatRole::User, "Keep the current release branch", None);
    let user_id = user.id.clone();
    let mut conflict = ChatMessage::new(ChatRole::System, "Planning changed", None);
    TaskChats::mark_planning_conflict_retry(&mut conflict);
    let mut first_window = TaskChats::default();
    first_window.ensure_loaded(slug);
    first_window
        .append(slug, key, vec![user, conflict])
        .unwrap();
    let first_claim = first_window
        .claim_pending_planning_conflict_retry(slug, key, &user_id, 1)
        .unwrap()
        .expect("first worker claims the one retry");

    expire_claim(slug, &first_claim);
    let mut second_window = TaskChats::default();
    second_window.ensure_loaded(slug);
    let second_claim = second_window
        .claim_pending_planning_conflict_retry(slug, key, &user_id, 1)
        .unwrap()
        .expect("one stale-lease takeover is allowed");
    assert_ne!(first_claim, second_claim);

    expire_claim(slug, &second_claim);
    let mut recovery_window = TaskChats::default();
    recovery_window.ensure_loaded(slug);
    assert_eq!(recovery_window.pending_planning_conflict_retries(key), None);
    assert_eq!(
        recovery_window.exhausted_planning_conflict_retry_claims(key),
        Some(1)
    );
    assert!(
        recovery_window
            .mark_planning_conflict_retry_exhausted(slug, key, &user_id, 1)
            .unwrap()
    );
    assert!(!recovery_window.has_pending_user_reply(key));
    assert!(recovery_window.has_failed_task_reply(key));

    let mut reopened = TaskChats::default();
    reopened.ensure_loaded(slug);
    assert!(!reopened.has_pending_user_reply(key));
    assert!(reopened.has_failed_task_reply(key));
    std::fs::remove_dir_all(dir).unwrap();
}

fn expire_claim(slug: &str, claim_id: &str) {
    let (mut stored, _) = super::super::super::read_stored(slug).unwrap();
    let messages = stored
        .other_histories
        .values_mut()
        .find(|messages| messages.iter().any(|message| message.id == claim_id))
        .unwrap();
    messages
        .iter_mut()
        .find(|message| message.id == claim_id)
        .unwrap()
        .ts = chrono::Utc::now() - chrono::Duration::minutes(16);
    let path = crate::persistence::project_dir(slug).join("task-conversations.json");
    let json = serde_json::to_string(&stored).unwrap();
    crate::artifacts::atomic_write(&path, &json).unwrap();
}
