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
fn capability_refresh_wait_is_durable_and_recovers_once() {
    let doc = doc(1, "None");
    let binding = crate::harness::CapabilityRefreshBindingV1 {
        provider_id: "pi".into(),
        role: crate::execution::contract::HarnessRole::Implementer,
        work_category: "implementation".into(),
        selected_model: "model-a".into(),
        requested_model: Some("model-a".into()),
        observed_at_unix_ms: 0,
    };
    let failure = Failure::awaiting_capability_refresh("refreshing selected CLI proof", binding);
    let mut queue = Queue::default();
    queue.auto_build = false;
    queue.blocked.insert(doc.path.clone(), failure.clone());
    queue
        .bind_task_documents(std::slice::from_ref(&doc))
        .unwrap();

    let mut restored: Queue =
        serde_json::from_str(&serde_json::to_string(&queue).unwrap()).unwrap();
    restored
        .bind_task_documents(std::slice::from_ref(&doc))
        .unwrap();
    let restored_failure = &restored.blocked[&doc.path];
    assert_eq!(restored_failure, &failure);
    assert_eq!(
        restored_failure.recovery,
        RecoveryDisposition::AwaitCapabilityRefresh
    );
    assert!(
        restored
            .recoverable_tickets(std::slice::from_ref(&doc))
            .is_empty()
    );

    restored.auto_build = true;
    assert_eq!(
        restored.recoverable_tickets(std::slice::from_ref(&doc)),
        vec![doc.path.clone()]
    );
    restored.schedule_recovery(std::slice::from_ref(&doc.path));
    assert!(
        restored
            .recoverable_tickets(std::slice::from_ref(&doc))
            .is_empty()
    );
}

#[test]
fn current_auto_publish_without_independent_checks_is_rejected() {
    let current = serde_json::json!({
        "schemaVersion": 6,
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
fn schema_five_queue_migrates_before_new_recovery_records_are_written() {
    let legacy = serde_json::json!({
        "schemaVersion": 5,
        "auto_plan": true,
        "auto_build": false,
        "auto_publish": false,
        "require_independent_checks": false,
        "running": false,
        "max_parallel": 3,
        "last_error": "",
        "recovery_paused": false,
        "tasks": {}
    });

    let queue = Queue::decode_persisted(&serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(queue).unwrap()["schemaVersion"], 6);
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

#[path = "tests/scheduling.rs"]
mod scheduling;
