use super::*;
use crate::{
    artifacts::task_docs::TaskDocument,
    core::implementation::{Failure, FailureKind, Implementation, RecoveryDisposition},
    domain::ArtifactIdentity,
};
use std::{collections::BTreeMap, fs};

fn story(path: &str, identity: ArtifactIdentity) -> TaskDocument {
    TaskDocument {
        path: path.into(),
        title: identity.title.clone(),
        text: format!("# {}", identity.title),
        identity: Some(identity),
        metadata: None,
        metadata_error: None,
    }
}

fn repo() -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("koolade-queue-identity-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    root
}

fn completed(path: &str) -> Implementation {
    serde_json::from_value(serde_json::json!({
        "ticket":path,"ticket_text":"","branch":"task","base":"main",
        "base_commit":"base","worktree":"fixture","status":"completed",
        "detail":"","pr_url":null,"verified_head":"head"
    }))
    .unwrap()
}

#[test]
fn blocked_retry_and_running_state_follow_uid_after_rename() {
    let root = repo();
    let old = ".koolade-packet/planning/tasks/current/001-login.md";
    let moved = ".koolade-packet/planning/tasks/renamed/009-login.md";
    let current = ArtifactIdentity::new("TASK-001", "Current story");
    let historical = ArtifactIdentity::new("TASK-001", "Historical reuse");
    let first = story(old, current.clone());
    let second = story(
        ".koolade-packet/planning/tasks/archive/001-old.md",
        historical.clone(),
    );
    let mut queue = Queue::default();
    for (doc, text) in [
        (&first, "preserve this blocker"),
        (&second, "keep the older blocker separate"),
    ] {
        queue.blocked.insert(
            doc.path.clone(),
            Failure::new(
                FailureKind::RemoteDiverged,
                RecoveryDisposition::AutomaticRetry,
                text,
            ),
        );
        queue.recovery_attempts.insert(doc.path.clone(), 1);
    }
    queue.current_ticket = Some(old.into());
    queue.in_flight.insert(old.into());
    assert!(
        queue
            .bind_task_documents(&[first.clone(), second.clone()])
            .unwrap()
    );
    queue.save(&root).unwrap();

    let path = directory(&root).unwrap().join("koolade-queue.json");
    let stored: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["schemaVersion"], 4);
    assert!(stored["tasks"].get(&current.uid).is_some());
    assert!(stored["tasks"].get(&historical.uid).is_some());
    assert!(
        stored.get("blocked").is_none(),
        "path-keyed blockers must not be persisted"
    );

    let moved_doc = story(moved, current.clone());
    assert!(
        queue
            .bind_task_documents(&[moved_doc.clone(), second.clone()])
            .unwrap()
    );
    assert!(!queue.blocked.contains_key(old));
    assert_eq!(
        queue.blocked[&moved_doc.path].message,
        "preserve this blocker"
    );
    assert_eq!(queue.recovery_attempts[&moved_doc.path], 1);
    assert_eq!(
        queue.current_ticket.as_deref(),
        Some(moved_doc.path.as_str())
    );
    assert!(queue.in_flight.contains(&moved_doc.path));
    queue.save(&root).unwrap();

    let mut reopened = Queue::load(&root).unwrap();
    reopened
        .bind_task_documents(&[moved_doc.clone(), second.clone()])
        .unwrap();
    assert_eq!(
        reopened.blocked[&moved_doc.path].message,
        "preserve this blocker"
    );
    assert_eq!(
        reopened.blocked[&second.path].message,
        "keep the older blocker separate"
    );
    assert_eq!(reopened.recovery_attempts[&moved_doc.path], 1);
    assert_eq!(
        reopened.current_ticket.as_deref(),
        Some(moved_doc.path.as_str())
    );
    assert!(reopened.in_flight.contains(&moved_doc.path));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reused_filename_orphans_old_state_instead_of_aliasing_it() {
    let root = repo();
    let path = ".koolade-packet/planning/tasks/current/001.md";
    let old_identity = ArtifactIdentity::new("TASK-001", "Old task");
    let new_identity = ArtifactIdentity::new("TASK-001", "New task");
    let old_doc = story(path, old_identity.clone());
    let new_doc = story(path, new_identity.clone());
    let mut queue = Queue::default();
    queue.blocked.insert(
        path.into(),
        Failure::new(
            FailureKind::RemoteDiverged,
            RecoveryDisposition::AutomaticRetry,
            "Old task failure",
        ),
    );
    queue.bind_task_documents(&[old_doc]).unwrap();
    queue.save(&root).unwrap();

    queue.bind_task_documents(&[new_doc]).unwrap();
    assert!(!queue.blocked.contains_key(path));
    assert_eq!(
        queue.blocked[&format!("@unlinked:{}", old_identity.uid)].message,
        "Old task failure"
    );
    queue.save(&root).unwrap();
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(directory(&root).unwrap().join("koolade-queue.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        value["tasks"][&old_identity.uid]["path_hint"],
        format!("@unlinked:{}", old_identity.uid)
    );
    assert!(value["tasks"].get(&new_identity.uid).is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn scheduler_uses_dependency_uids_even_when_markdown_links_change() {
    let batch_uid = uuid::Uuid::new_v4().to_string();
    let mut blocked_identity = ArtifactIdentity::new("TASK-001", "Dependent task");
    blocked_identity.parent_uid = Some(batch_uid.clone());
    let dependency_identity = ArtifactIdentity {
        uid: uuid::Uuid::new_v4().to_string(),
        display_id: "TASK-002".into(),
        title: "Dependency".into(),
        parent_uid: Some(batch_uid),
    };
    let blocked_path = ".koolade-packet/planning/tasks/batch/001-dependent.md";
    let dependency_path = ".koolade-packet/planning/tasks/batch/002-dependency.md";
    let blocked_metadata = crate::artifacts::task_docs::TaskMetadata::new(
        &blocked_identity,
        "root",
        vec![dependency_identity.uid.clone()],
    )
    .unwrap();
    let dependency_metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&dependency_identity, "root", vec![])
            .unwrap();
    let docs = vec![
        TaskDocument {
            path: blocked_path.into(),
            title: "Dependent task".into(),
            text: "# Dependent task\n\n## Dependencies\n\nNone.".into(),
            identity: Some(blocked_identity),
            metadata: Some(blocked_metadata),
            metadata_error: None,
        },
        TaskDocument {
            path: dependency_path.into(),
            title: "Dependency".into(),
            text: "# Dependency\n\n## Dependencies\n\nNone.".into(),
            identity: Some(dependency_identity),
            metadata: Some(dependency_metadata),
            metadata_error: None,
        },
    ];
    let mut states = BTreeMap::new();
    assert!(
        ticket_readiness(&docs, &states, blocked_path)
            .unwrap_err()
            .contains("waiting")
    );
    assert_eq!(
        next_ready_ticket(&docs, &states, &Default::default()).unwrap(),
        Some(dependency_path.into()),
        "the rendered Markdown says independent, but metadata says this task must wait"
    );
    states.insert(dependency_path.into(), completed(dependency_path));
    assert!(ticket_readiness(&docs, &states, blocked_path).is_ok());
    assert_eq!(
        next_ready_ticket(&docs, &states, &Default::default()).unwrap(),
        Some(blocked_path.into())
    );
}
