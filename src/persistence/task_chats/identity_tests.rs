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
        task_state: None,

        metadata_error: None,
    }
}

#[test]
fn unversioned_task_chat_history_is_rejected_without_rewrite() {
    let dir = std::env::temp_dir().join(format!("koolade_task_identity_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let slug = dir.to_str().unwrap();
    let old_path = "planning/tasks/first/001-login.md";
    let old_format = BTreeMap::from([(
        old_path.to_owned(),
        vec![ChatMessage::new(ChatRole::User, "Old conversation", None)],
    )]);
    let original = serde_json::to_vec(&old_format).unwrap();
    let store_path = dir.join("task-conversations.json");
    std::fs::write(&store_path, &original).unwrap();

    assert!(read_stored(slug).is_err());
    let mut chats = TaskChats::default();
    chats.ensure_loaded(slug);
    assert!(chats.error.is_some());
    assert_eq!(std::fs::read(store_path).unwrap(), original);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unsupported_versioned_task_chat_store_is_rejected_without_rewrite() {
    let dir = std::env::temp_dir().join(format!(
        "koolade_task_identity_version_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let slug = dir.to_str().unwrap();
    let mut value = serde_json::to_value(super::storage::StoredHistories::default()).unwrap();
    value["schemaVersion"] = serde_json::Value::from(1);
    let original = serde_json::to_vec(&value).unwrap();
    let store_path = dir.join("task-conversations.json");
    std::fs::write(&store_path, &original).unwrap();

    assert!(read_stored(slug).is_err());
    assert_eq!(std::fs::read(store_path).unwrap(), original);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn duplicate_display_ids_keep_separate_task_conversations() {
    let dir =
        std::env::temp_dir().join(format!("koolade_duplicate_task_ids_{}", std::process::id()));
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
    let stored = read_stored(slug).unwrap();
    assert_eq!(stored.task_histories.len(), 2);
    assert_ne!(first.uid, second.uid);
    std::fs::remove_dir_all(dir).unwrap();
}
