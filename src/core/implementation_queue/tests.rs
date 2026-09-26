use crate::artifacts::task_docs::TaskDocument;
use crate::core::implementation::{Failure, FailureKind, Implementation, RecoveryDisposition};
use std::{collections::BTreeMap, fs};

use super::*;

fn doc(n: usize, dependencies: &str) -> TaskDocument {
    TaskDocument {
        path: format!(".kool-ade-packet/planning/tasks/fixture/{n:03}-task.md"),
        title: format!("Task {n}"),
        text: format!(
            "# Task {n}\n\n## Dependencies\n{dependencies}\n\n## Acceptance criteria\n- Done."
        ),
        identity: None,
        metadata: None,
        metadata_error: None,
    }
}

fn done(ticket: &str) -> Implementation {
    serde_json::from_value(serde_json::json!({"ticket":ticket,"ticket_text":"","branch":"task","base":"main","base_commit":"base","worktree":"fixture","status":"completed","detail":"","pr_url":null,"verified_head":"head"})).unwrap()
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
    let restored: Queue = serde_json::from_str(&serde_json::to_string(&queue).unwrap()).unwrap();
    assert!(restored.recoverable_tickets(&docs).is_empty());
    assert!(restored.blocked.contains_key(&docs[1].path));
}

#[test]
fn legacy_queue_errors_are_typed_once_and_future_versions_reject_prose_values() {
    let old = serde_json::json!({"blocked":{"ticket":"Local branch diverged before publication"}});
    let (queue, migrated) = Queue::decode_persisted(&serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(migrated);
    assert_eq!(queue.blocked["ticket"].kind, FailureKind::RemoteDiverged);
    assert_eq!(
        queue.blocked["ticket"].recovery,
        RecoveryDisposition::AutomaticRetry
    );

    let current = serde_json::json!({"schemaVersion":1,"blocked":{"ticket":"untyped error"}});
    assert!(Queue::decode_persisted(&serde_json::to_vec(&current).unwrap()).is_err());
}

#[test]
fn version_two_auto_mode_migrates_to_separate_policy_without_granting_auto_publish() {
    let old = serde_json::json!({
        "schemaVersion": 2,
        "auto_mode": true,
        "running": true,
        "max_parallel": 4,
        "tasks": {
            "task-uid": {
                "path_hint": "tasks/001-task.md",
                "current": true,
                "in_flight": true,
                "blocked": null,
                "recovery_attempts": 0
            }
        },
        "legacy_tasks": {},
        "task_identity_aliases": {"tasks/001-task.md": "task-uid"}
    });
    let (queue, migrated) = Queue::decode_persisted(&serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(migrated);
    assert!(queue.auto_plan && queue.auto_build);
    assert!(!queue.auto_publish);
    assert!(queue.running && queue.in_flight.contains("tasks/001-task.md"));

    let saved = serde_json::to_value(&queue).unwrap();
    assert_eq!(saved["schemaVersion"], 4);
    assert_eq!(saved["tasks"]["task-uid"]["in_flight"], true);
    assert_eq!(
        saved["task_identity_aliases"]["tasks/001-task.md"],
        "task-uid"
    );
    assert_eq!(saved["auto_publish"], false);
    assert_eq!(saved["require_independent_checks"], false);
}

#[test]
fn version_three_queue_migration_keeps_publication_policy_conservative() {
    let old = serde_json::json!({
        "schemaVersion": 3,
        "auto_plan": true,
        "auto_build": false,
        "auto_publish": true,
        "running": true,
        "max_parallel": 2,
        "tasks": {},
        "legacy_tasks": {},
        "task_identity_aliases": {}
    });
    let (queue, migrated) = Queue::decode_persisted(&serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(migrated);
    assert!(queue.auto_publish);
    assert!(!queue.auto_build);
    assert!(!queue.require_independent_checks);
}

#[test]
fn loading_legacy_queue_rewrites_typed_failure_state_atomically() {
    let root = std::env::temp_dir().join(format!(
        "packet-legacy-queue-{}",
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
    let path = directory(&root).unwrap().join("packet-queue.json");
    fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "blocked":{"task":"Local and origin diverged before publication"}
        }))
        .unwrap(),
    )
    .unwrap();

    let loaded = Queue::load(&root).unwrap();
    assert_eq!(
        loaded.blocked["task"].recovery,
        RecoveryDisposition::AutomaticRetry
    );
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["schemaVersion"], 4);
    assert_eq!(
        saved["legacy_tasks"]["task"]["blocked"]["kind"],
        "remote_diverged"
    );
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
        waiting.contains("waiting") && waiting.contains("unsupported"),
        "unexpected dependency explanation: {waiting}"
    );
}

#[test]
fn preferences_and_inflight_ticket_survive_restart_and_lock_excludes_another_window() {
    let root = std::env::temp_dir().join(format!(
        "packet-queue-{}",
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
    queue.running = true;
    queue.current_ticket = Some(".kool-ade-packet/planning/tasks/fixture/001-task.md".into());
    queue.in_flight.extend(["one".into(), "two".into()]);
    queue.max_parallel = 4;
    queue.save(&root).unwrap();
    drop(lock);
    let _new_lock = Queue::acquire(&root).unwrap();
    let loaded = Queue::load(&root).unwrap();
    assert!(loaded.running && loaded.auto_build);
    assert!(loaded.auto_plan && !loaded.auto_publish);
    assert_eq!(loaded.current_ticket, queue.current_ticket);
    assert_eq!(loaded.in_flight, queue.in_flight);
    assert_eq!(loaded.max_parallel, 4);
    drop(_new_lock);
    fs::remove_dir_all(root).unwrap();
}
