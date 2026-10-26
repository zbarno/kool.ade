use crate::domain::{ChatMessage, ChatRole};

use super::store::{append_at_for_test, load_at_for_test};

fn message(role: ChatRole, text: &str) -> ChatMessage {
    ChatMessage::new(role, text, None)
}

#[test]
fn histories_survive_reopen_and_remain_bound_to_each_setup_task() {
    let directory = tempfile::tempdir().unwrap();
    let first = append_at_for_test(
        directory.path(),
        "setup:docker:rootless",
        &[message(ChatRole::User, "How do I check this?")],
    )
    .unwrap();
    assert_eq!(first.len(), 1);
    append_at_for_test(
        directory.path(),
        "setup:cli:scan",
        &[message(ChatRole::User, "Check my coding agent")],
    )
    .unwrap();

    let reopened = load_at_for_test(directory.path(), "setup:docker:rootless").unwrap();
    let separate = load_at_for_test(directory.path(), "setup:cli:scan").unwrap();
    assert_eq!(reopened[0].text, "How do I check this?");
    assert_eq!(separate[0].text, "Check my coding agent");
    assert_ne!(reopened[0].id, separate[0].id);
}

#[test]
fn invalid_task_ids_and_oversized_messages_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    assert!(load_at_for_test(directory.path(), "setup:../escape").is_err());
    assert!(load_at_for_test(directory.path(), &format!("setup:{}", "x".repeat(180))).is_err());
    assert!(
        append_at_for_test(
            directory.path(),
            "setup:docker:rootless",
            &[message(ChatRole::User, &"x".repeat(16 * 1024 + 1))],
        )
        .is_err()
    );
}

#[test]
fn schema_and_task_identity_are_validated_and_history_is_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let task_id = "setup:docker:rootless";
    let oversized_history = (0..12)
        .map(|index| {
            message(
                ChatRole::User,
                &format!("{index} {}", "x".repeat(15 * 1024)),
            )
        })
        .collect::<Vec<_>>();
    let bounded = append_at_for_test(directory.path(), task_id, &oversized_history).unwrap();
    assert!(bounded.len() < oversized_history.len());
    assert!(bounded.iter().map(|item| item.text.len()).sum::<usize>() <= 128 * 1024);
    let many_messages = (0..105)
        .map(|index| message(ChatRole::User, &format!("turn {index}")))
        .collect::<Vec<_>>();
    let count_bounded =
        append_at_for_test(directory.path(), "setup:cli:scan", &many_messages).unwrap();
    assert_eq!(count_bounded.len(), 100);

    let file = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            if !path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                return false;
            }
            std::fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|record| record["identity"]["taskId"].as_str().map(str::to_owned))
                .as_deref()
                == Some(task_id)
        })
        .unwrap();
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    record["schemaVersion"] = serde_json::json!(2);
    std::fs::write(&file, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(load_at_for_test(directory.path(), task_id).is_err());

    record["schemaVersion"] = serde_json::json!(1);
    record["identity"]["taskId"] = serde_json::json!("setup:cli:scan");
    std::fs::write(&file, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(load_at_for_test(directory.path(), task_id).is_err());
}

#[test]
fn persisted_messages_redact_credentials_and_keep_only_bounded_history() {
    let directory = tempfile::tempdir().unwrap();
    let task_id = "setup:docker:rootless";
    let stored = append_at_for_test(
        directory.path(),
        task_id,
        &[message(
            ChatRole::User,
            "token=synthetic-token https://alice:synthetic-password@example.invalid/help",
        )],
    )
    .unwrap();
    assert!(!stored[0].text.contains("synthetic-token"));
    assert!(!stored[0].text.contains("synthetic-password"));

    let state_file = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .unwrap();
    let bytes = std::fs::read(&state_file).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("synthetic-token"));
    assert!(!text.contains("synthetic-password"));
    assert!(text.contains(task_id));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(state_file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(directory.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn concurrent_appends_to_one_task_do_not_lose_messages() {
    let directory = tempfile::tempdir().unwrap();
    let task_id = "setup:docker:rootless".to_owned();
    let writers = (0..8)
        .map(|index| {
            let path = directory.path().to_path_buf();
            let task_id = task_id.clone();
            std::thread::spawn(move || {
                append_at_for_test(
                    &path,
                    &task_id,
                    &[message(ChatRole::User, &format!("message {index}"))],
                )
                .unwrap();
            })
        })
        .collect::<Vec<_>>();
    for writer in writers {
        writer.join().unwrap();
    }
    let history = load_at_for_test(directory.path(), &task_id).unwrap();
    assert_eq!(history.len(), 8);
}
