use super::storage::read_stored;
use crate::artifacts::task_docs::TaskDocument;
use crate::domain::{ChatMessage, ChatRole};
use std::collections::BTreeMap;

use super::*;

fn story(path: &str, identity: crate::domain::ArtifactIdentity) -> TaskDocument {
    TaskDocument {
        path: path.into(),
        title: identity.title.clone(),
        text: format!("# {}", identity.title),
        identity: Some(identity),
        metadata: None,
        metadata_error: None,
    }
}

#[test]
fn legacy_history_migrates_to_uid_and_follows_a_task_move() {
    let dir = std::env::temp_dir().join(format!("packet_task_identity_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let slug = dir.to_str().unwrap();
    let old_path = ".kool-ade-packet/planning/tasks/first/001-login.md";
    let new_path = ".kool-ade-packet/planning/tasks/renamed/009-login.md";
    let identity = crate::domain::ArtifactIdentity::new("TASK-001", "Login");
    let original = ChatMessage::new(ChatRole::User, "Keep the SSO decision", None);
    let legacy = BTreeMap::from([(old_path.to_owned(), vec![original.clone()])]);
    std::fs::write(
        dir.join("task-conversations.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();

    let mut chats = TaskChats::default();
    chats
        .bind_task_documents(&[story(old_path, identity.clone())])
        .unwrap();
    chats.ensure_loaded(slug);
    assert_eq!(chats.messages[old_path], vec![original.clone()]);
    let (stored, _) = read_stored(slug).unwrap();
    assert_eq!(stored.schema_version, 2);
    assert_eq!(stored.task_histories[&identity.uid], vec![original.clone()]);
    assert!(!stored.other_histories.contains_key(old_path));

    chats
        .bind_task_documents(&[story(new_path, identity.clone())])
        .unwrap();
    assert!(!chats.messages.contains_key(old_path));
    assert_eq!(chats.messages[new_path], vec![original.clone()]);
    let answer = ChatMessage::new(
        ChatRole::Agent,
        "The stored decision is still available",
        None,
    );
    chats.append(slug, new_path, vec![answer.clone()]).unwrap();

    let mut reopened = TaskChats::default();
    reopened
        .bind_task_documents(&[story(new_path, identity.clone())])
        .unwrap();
    reopened.ensure_loaded(slug);
    assert_eq!(reopened.messages[new_path], vec![original, answer]);
    let (stored, _) = read_stored(slug).unwrap();
    assert_eq!(stored.task_histories.len(), 1);
    assert!(stored.task_histories.contains_key(&identity.uid));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn duplicate_display_ids_keep_separate_task_conversations() {
    let dir =
        std::env::temp_dir().join(format!("packet_duplicate_task_ids_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let slug = dir.to_str().unwrap();
    let first = crate::domain::ArtifactIdentity::new("TASK-001", "First story");
    let second = crate::domain::ArtifactIdentity::new("TASK-001", "Historical reuse");
    let docs = [
        story("planning/tasks/first/001.md", first.clone()),
        story("planning/tasks/second/001.md", second.clone()),
    ];
    let mut chats = TaskChats::default();
    chats.bind_task_documents(&docs).unwrap();
    chats
        .append(
            slug,
            &docs[0].path,
            vec![ChatMessage::new(ChatRole::User, "First decision", None)],
        )
        .unwrap();
    chats
        .append(
            slug,
            &docs[1].path,
            vec![ChatMessage::new(ChatRole::User, "Second decision", None)],
        )
        .unwrap();

    let mut reopened = TaskChats::default();
    reopened.bind_task_documents(&docs).unwrap();
    reopened.ensure_loaded(slug);
    assert_eq!(reopened.messages[&docs[0].path][0].text, "First decision");
    assert_eq!(reopened.messages[&docs[1].path][0].text, "Second decision");
    let (stored, _) = read_stored(slug).unwrap();
    assert_eq!(stored.task_histories.len(), 2);
    assert_ne!(first.uid, second.uid);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn reused_filename_keeps_old_conversation_unlinked_from_new_task() {
    let dir = std::env::temp_dir().join(format!("packet_reused_chat_path_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let slug = dir.to_str().unwrap();
    let path = ".kool-ade-packet/planning/tasks/current/001.md";
    let old_identity = crate::domain::ArtifactIdentity::new("TASK-001", "Old task");
    let new_identity = crate::domain::ArtifactIdentity::new("TASK-001", "New task");
    let legacy = BTreeMap::from([(
        path.to_owned(),
        vec![ChatMessage::new(ChatRole::User, "Old answer", None)],
    )]);
    std::fs::write(
        dir.join("task-conversations.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();

    let mut chats = TaskChats::default();
    chats
        .bind_task_documents(&[story(path, old_identity.clone())])
        .unwrap();
    chats.ensure_loaded(slug);
    chats
        .bind_task_documents(&[story(path, new_identity.clone())])
        .unwrap();
    assert!(chats.messages[&format!("@unlinked:{}", old_identity.uid)][0].text == "Old answer");
    chats
        .append(
            slug,
            path,
            vec![ChatMessage::new(ChatRole::User, "New answer", None)],
        )
        .unwrap();
    assert_eq!(chats.messages[path][0].text, "New answer");
    let (stored, _) = read_stored(slug).unwrap();
    assert_eq!(stored.task_identities[path], old_identity.uid);
    assert_eq!(stored.task_histories.len(), 2);
    assert!(stored.task_histories.contains_key(&new_identity.uid));
    std::fs::remove_dir_all(dir).unwrap();
}
