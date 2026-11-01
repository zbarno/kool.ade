use super::{TaskChats, read};
use crate::domain::{ChatMessage, ChatRole};

#[path = "tests/ambiguous_identity.rs"]
mod ambiguous_identity;
#[path = "tests/context_routing.rs"]
mod context_routing;
#[path = "tests/identity_routing.rs"]
mod identity_routing;
#[path = "tests/reply_state.rs"]
mod reply_state;
#[path = "tests/storage_recovery.rs"]
mod storage_recovery;

#[test]
fn project_context_keeps_task_identity_and_prioritizes_explicit_task_ids() {
    let mut chats = TaskChats::default();
    chats.messages.insert(
        "planning/tasks/feature/001-login.md".into(),
        vec![
            ChatMessage::new(ChatRole::User, "Use corporate SSO", None),
            ChatMessage::new(ChatRole::System, "Task reply applied. Resolved.", None),
        ],
    );
    chats.messages.insert(
        "CLR-002".into(),
        vec![ChatMessage::new(ChatRole::User, "Require audit logs", None)],
    );
    let context = chats.project_context("What happened in TASK-001?", 24000);
    for fact in [
        "001-login.md",
        "Use corporate SSO",
        "Resolved.",
        "CLR-002",
        "Require audit logs",
    ] {
        assert!(context.contains(fact));
    }
    assert!(context.find("001-login.md").unwrap() < context.find("CLR-002").unwrap());
}

#[test]
fn focused_project_context_excludes_unmentioned_conversations() {
    let mut chats = TaskChats::default();
    chats.messages.insert(
        "CLR-001".into(),
        vec![ChatMessage::new(ChatRole::User, "Use corporate SSO", None)],
    );
    chats.messages.insert(
        "CLR-002".into(),
        vec![ChatMessage::new(ChatRole::User, "Require audit logs", None)],
    );

    let focused = chats.project_context_for_focus("Use the answer in CLR-001", 24000);
    assert!(focused.contains("Use corporate SSO"));
    assert!(!focused.contains("Require audit logs"));
    assert!(
        chats
            .project_context_for_focus("Plan another feature", 24000)
            .is_empty()
    );
}

#[test]
fn refresh_reports_other_window_interactions_once() {
    let dir =
        std::env::temp_dir().join(format!("koolade_task_notifications_{}", std::process::id()));
    let slug = dir.to_str().unwrap();
    let mut main = TaskChats::default();
    main.ensure_loaded(slug);
    let mut other = TaskChats::default();
    other
        .append(
            slug,
            "CLR-001",
            vec![ChatMessage::new(ChatRole::User, "Use SSO", None)],
        )
        .unwrap();
    main.ensure_loaded(slug);
    assert!(
        !main.has_pending_user_reply("CLR-001"),
        "the routine refresh is intentionally throttled within its short cache window"
    );
    main.refresh_now(slug);
    assert!(main.has_pending_user_reply("CLR-001"));
    let updates = main.take_updates();
    assert_eq!(updates.len(), 1);
    assert!(updates[0].contains("CLR-001"));
    assert!(updates[0].contains("Use SSO"));
    main.refresh_now(slug);
    assert!(main.take_updates().is_empty());
    assert!(main.project_context("", 24000).contains("Use SSO"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn independent_streams_survive_restart_without_compaction_or_main_chat() {
    let dir = std::env::temp_dir().join(format!("koolade_task_chats_{}", std::process::id()));
    let slug = dir.to_str().unwrap();
    let mut chats = TaskChats::default();
    let first = (0..510)
        .map(|i| ChatMessage::new(ChatRole::User, format!("reply {i}"), None))
        .collect::<Vec<_>>();
    chats.append(slug, "CLR-001", first.clone()).unwrap();
    chats
        .append(
            slug,
            "planning/tasks/002.md",
            vec![ChatMessage::new(ChatRole::User, "other task", None)],
        )
        .unwrap();
    let mut reopened = TaskChats::default();
    reopened.ensure_loaded(slug);
    assert_eq!(reopened.messages["CLR-001"], first);
    assert_eq!(reopened.messages["planning/tasks/002.md"].len(), 1);
    assert!(!dir.join("chat.jsonl").exists());
    std::fs::remove_dir_all(dir).unwrap();
}
