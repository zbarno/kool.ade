use crate::artifacts::task_docs::TaskDocument;
use crate::core::implementation::{Failure, FailureKind, Implementation, RecoveryDisposition};
use std::{collections::BTreeMap, fs};

use super::*;

fn doc(n: usize, dependencies: &str) -> TaskDocument {
    let mut identity =
        crate::domain::ArtifactIdentity::new(&format!("TASK-{n:03}"), &format!("Task {n}"));
    identity.uid = uuid::Uuid::from_u128(n as u128).to_string();
    identity.parent_uid = Some(uuid::Uuid::from_u128(10_000).to_string());
    let dependency_uids = dependencies
        .split("](")
        .skip(1)
        .filter_map(|link| link.split(')').next())
        .filter_map(|target| {
            let target = target.rsplit('/').next()?;
            Some(
                target
                    .split('-')
                    .next()?
                    .parse::<u128>()
                    .ok()
                    .unwrap_or(99_999),
            )
        })
        .map(|n| uuid::Uuid::from_u128(n).to_string())
        .collect();
    let metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", dependency_uids).unwrap();
    TaskDocument {
        path: format!("planning/tasks/fixture/{n:03}-task.md"),
        title: format!("Task {n}"),
        text: format!(
            "# Task {n}\n\n## Dependencies\n{dependencies}\n\n## Acceptance criteria\n- Done."
        ),
        identity: Some(identity),
        metadata: Some(metadata),
        task_state: None,

        metadata_error: None,
    }
}

fn done(ticket: &str) -> Implementation {
    serde_json::from_value(serde_json::json!({"ticket":ticket,"ticket_text":"","branch":"task","base":"main","base_commit":"base","task_repository":"fixture","task_repository_kind":"clone","status":"completed","detail":"","pr_url":null,"verified_head":"head"})).unwrap()
}

#[test]
fn unannotated_dependency_markdown_is_not_used_by_the_scheduler() {
    let mut task = doc(1, "- [Task 002](002-task.md)");
    task.metadata = None;

    let error = super::task_dependencies(&task, &[]).unwrap_err();

    assert!(error.contains("lacks current Kool.ad/e task metadata"));
}

fn task_doc(
    path: &str,
    uid: &str,
    batch_uid: &str,
    parent_task_uid: Option<&str>,
    dependencies: Vec<String>,
) -> TaskDocument {
    let mut identity = crate::domain::ArtifactIdentity::new(uid, path);
    identity.uid = uid.to_owned();
    identity.parent_uid = Some(batch_uid.to_owned());
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", dependencies)
        .unwrap()
        .with_parent_task_uid(parent_task_uid)
        .unwrap();
    TaskDocument {
        path: path.into(),
        title: path.into(),
        text: format!("# {path}\n\n## Dependencies\nNone.\n"),
        identity: Some(identity),
        metadata: Some(metadata),
        task_state: None,
        metadata_error: None,
    }
}

#[test]
fn standalone_parent_waits_for_subtask_lifecycle_completion_and_siblings_follow_dag() {
    let batch_uid = "00000000-0000-4000-8000-000000000010";
    let parent_uid = "00000000-0000-4000-8000-000000000011";
    let first_uid = "00000000-0000-4000-8000-000000000012";
    let second_uid = "00000000-0000-4000-8000-000000000013";
    let parent = task_doc("parent.md", parent_uid, batch_uid, None, vec![]);
    let first = task_doc("first.md", first_uid, batch_uid, Some(parent_uid), vec![]);
    let second = task_doc(
        "second.md",
        second_uid,
        batch_uid,
        Some(parent_uid),
        vec![first_uid.into()],
    );
    let docs = vec![parent.clone(), first.clone(), second.clone()];
    let mut first_state = done(&first.path);
    first_state.status = crate::core::implementation::ImplementationStatus::AwaitingApproval;
    first_state.merged_commit = None;
    let mut states = BTreeMap::from([(first.path.clone(), first_state.clone())]);
    assert!(
        ticket_readiness(&docs, &states, &parent.path)
            .unwrap_err()
            .contains("subtask")
    );

    let completed_first = std::collections::BTreeSet::from([first_uid.to_owned()]);
    assert!(
        ticket_readiness_with_lifecycle_subtasks(&docs, &states, &second.path, &completed_first,)
            .is_ok()
    );
    assert!(
        ticket_readiness_with_lifecycle_subtasks(&docs, &states, &parent.path, &completed_first,)
            .unwrap_err()
            .contains("second.md")
    );

    states.insert(second.path.clone(), first_state);
    let completed = std::collections::BTreeSet::from([first_uid.into(), second_uid.into()]);
    assert!(
        ticket_readiness_with_lifecycle_subtasks(&docs, &states, &parent.path, &completed).is_ok()
    );
}

#[test]
fn known_orchestration_failures_recover_once_but_external_blockers_do_not() {
    let docs = vec![doc(1, "None"), doc(2, "None")];
    let mut queue = Queue::default();
    queue.blocked.insert(
        docs[0].path.clone(),
        Failure::new(
            FailureKind::RemoteDiverged,
            RecoveryDisposition::AutomaticRetry,
            "Local main and freshly fetched origin/main diverged before publication; verified work is preserved",
        ),
    );
    queue.blocked.insert(
        docs[1].path.clone(),
        Failure::new(
            FailureKind::ExternalPrerequisite,
            RecoveryDisposition::UserAction,
            "Credentials require human intervention",
        ),
    );
    queue.recovery_paused = true;
    assert!(queue.recoverable_tickets(&docs).is_empty());
    queue.recovery_paused = false;
    let tickets = queue.recoverable_tickets(&docs);
    assert_eq!(tickets, vec![docs[0].path.clone()]);
    queue.schedule_recovery(&tickets);
    assert!(queue.running);
    queue.bind_task_documents(&docs).unwrap();
    let mut restored: Queue =
        serde_json::from_str(&serde_json::to_string(&queue).unwrap()).unwrap();
    restored.bind_task_documents(&docs).unwrap();
    assert!(restored.recoverable_tickets(&docs).is_empty());
    assert!(restored.blocked.contains_key(&docs[1].path));
}

#[test]
fn current_auto_publish_without_independent_checks_is_rejected() {
    let current = serde_json::json!({
        "schemaVersion": 5,
        "auto_plan": true,
        "auto_build": false,
        "auto_publish": true,
        "require_independent_checks": false,
        "running": false,
        "max_parallel": 3,
        "last_error": "",
        "recovery_paused": false,
        "tasks": {}
    });
    assert!(Queue::decode_persisted(&serde_json::to_vec(&current).unwrap()).is_err());
}

#[test]
fn loading_unsupported_queue_preserves_the_original_bytes() {
    let root = std::env::temp_dir().join(format!(
        "koolade-unsupported-queue-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let path = directory(&root).unwrap().join("koolade-queue.json");
    let original = br#"{"blocked":{"task":"Local and origin diverged before publication"}}"#;
    fs::write(&path, original).unwrap();

    assert!(Queue::load(&root).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn queue_advances_in_order_and_waits_for_dependencies() {
    let docs = vec![
        doc(2, "- [Task 001](001-task.md) must be complete."),
        doc(1, "None."),
    ];
    let mut states = BTreeMap::new();
    assert!(
        ticket_readiness(&docs, &states, &docs[0].path)
            .unwrap_err()
            .contains("waiting")
    );
    assert_eq!(
        next_ticket(&docs, &states).unwrap(),
        Some(docs[1].path.clone())
    );
    states.insert(docs[1].path.clone(), done(&docs[1].path));
    assert!(ticket_readiness(&docs, &states, &docs[0].path).is_ok());
    assert_eq!(
        next_ticket(&docs, &states).unwrap(),
        Some(docs[0].path.clone())
    );
    states.insert(docs[0].path.clone(), done(&docs[0].path));
    assert_eq!(next_ticket(&docs, &states).unwrap(), None);
    assert!(
        next_ticket(&[doc(1, "- [missing](009-task.md)")], &BTreeMap::new())
            .unwrap_err()
            .contains("waiting")
    );
}

#[test]
fn automatic_build_queue_leaves_verified_unpublished_work_for_user_action() {
    let docs = vec![doc(1, "None")];
    let mut state = done(&docs[0].path);
    state.status = crate::core::implementation::ImplementationStatus::ReadyToPublish;
    state.merged_commit = None;
    let states = BTreeMap::from([(docs[0].path.clone(), state)]);

    let waiting = next_ready_ticket(&docs, &states, &Default::default()).unwrap_err();
    assert!(waiting.contains("verified") && waiting.contains("waiting for you to publish"));
}

#[test]
fn independent_tasks_skip_blocked_and_running_predecessors() {
    let docs = vec![
        doc(1, "- [missing](009-task.md)"),
        doc(2, "None."),
        doc(3, "None."),
        doc(4, "- [two](002-task.md)"),
    ];
    let states = BTreeMap::new();
    assert_eq!(
        next_ticket(&docs, &states).unwrap(),
        Some(docs[1].path.clone())
    );
    let active = std::collections::BTreeSet::from([docs[1].path.clone()]);
    assert_eq!(
        next_ready_ticket(&docs, &states, &active).unwrap(),
        Some(docs[2].path.clone())
    );
    let active = std::collections::BTreeSet::from([docs[1].path.clone(), docs[2].path.clone()]);
    assert!(
        next_ready_ticket(&docs, &states, &active)
            .unwrap_err()
            .contains("waiting")
    );
    let states = BTreeMap::from([(docs[1].path.clone(), done(&docs[1].path))]);
    assert_eq!(
        next_ready_ticket(&docs, &states, &active).unwrap(),
        Some(docs[3].path.clone())
    );
}

#[test]
fn cycles_and_invalid_dependencies_do_not_starve_independent_work() {
    let docs = vec![
        doc(1, "- [two](002-task.md)"),
        doc(2, "- [one](001-task.md)"),
        doc(3, "- [unsafe](../other.md)"),
        doc(4, "None."),
    ];
    assert_eq!(
        next_ticket(&docs, &BTreeMap::new()).unwrap(),
        Some(docs[3].path.clone())
    );
    let waiting = next_ticket(&docs[..3], &BTreeMap::new()).unwrap_err();
    assert!(
        waiting.contains("waiting") && waiting.contains("identity"),
        "unexpected dependency explanation: {waiting}"
    );
}

#[test]
fn preferences_and_inflight_ticket_survive_restart_and_lock_excludes_another_window() {
    let root = std::env::temp_dir().join(format!(
        "koolade-queue-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let mut queue = Queue::load(&root).unwrap();
    assert!(queue.auto_plan);
    assert!(queue.auto_build);
    assert!(!queue.auto_publish);
    assert!(!queue.running);
    let lock = Queue::acquire(&root).unwrap();
    assert!(Queue::acquire(&root).is_err());
    let docs = vec![doc(1, "None."), doc(2, "None.")];
    queue.bind_task_documents(&docs).unwrap();
    queue.running = true;
    queue.current_ticket = Some(docs[0].path.clone());
    queue
        .in_flight
        .extend(docs.iter().map(|doc| doc.path.clone()));
    queue.max_parallel = 4;
    queue.save(&root).unwrap();
    drop(lock);
    let _new_lock = Queue::acquire(&root).unwrap();
    let mut loaded = Queue::load(&root).unwrap();
    loaded.bind_task_documents(&docs).unwrap();
    assert!(loaded.running && loaded.auto_build);
    assert!(loaded.auto_plan && !loaded.auto_publish);
    assert_eq!(loaded.current_ticket, queue.current_ticket);
    assert_eq!(loaded.in_flight, queue.in_flight);
    assert_eq!(loaded.max_parallel, 4);
    drop(_new_lock);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn loading_after_a_controller_restart_clears_saved_wall_clock_queue_age() {
    let root = std::env::temp_dir().join(format!(
        "koolade-queue-restart-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let mut queue = Queue::load(&root).unwrap();
    let docs = vec![doc(1, "None.")];
    queue.bind_task_documents(&docs).unwrap();
    assert!(queue.update_ready_waits(&[docs[0].path.clone()], 100, true));
    queue.save(&root).unwrap();

    let mut restored = Queue::load(&root).unwrap();
    restored.bind_task_documents(&docs).unwrap();
    assert_eq!(restored.take_queue_wait_ms(&docs[0].path, 10_000), None);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn queue_load_does_not_rewrite_age_while_another_controller_holds_the_lock() {
    let root = std::env::temp_dir().join(format!(
        "koolade-queue-owned-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let mut queue = Queue::load(&root).unwrap();
    let docs = vec![doc(1, "None.")];
    queue.bind_task_documents(&docs).unwrap();
    assert!(queue.update_ready_waits(&[docs[0].path.clone()], 100, true));
    queue.save(&root).unwrap();

    let active_lock = Queue::acquire(&root).unwrap();
    let mut restored = Queue::load(&root).unwrap();
    let queue_path = directory(&root).unwrap().join("koolade-queue.json");
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&queue_path).unwrap()).unwrap();
    let task_uid = docs[0].identity.as_ref().unwrap().uid.as_str();
    assert_eq!(stored["tasks"][task_uid]["waiting_since_unix_ms"], 100);
    drop(active_lock);
    assert_eq!(restored.take_queue_wait_ms(&docs[0].path, 10_000), None);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn runnable_ticket_collection_returns_all_eligible_work() {
    let docs = vec![doc(3, "None."), doc(1, "None."), doc(2, "None.")];
    let ready = ready_tickets_with_lifecycle_subtasks(
        &docs,
        &BTreeMap::new(),
        &Default::default(),
        &Default::default(),
        &Default::default(),
    );
    assert_eq!(
        ready,
        vec![
            docs[1].path.clone(),
            docs[2].path.clone(),
            docs[0].path.clone()
        ]
    );
}
