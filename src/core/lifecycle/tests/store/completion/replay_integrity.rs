use super::super::Fixture;
use super::support;
use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt, LifecycleStore, TaskEvent};

fn completed_parent_with_pr(
    fixture: &Fixture,
    lifecycle: &LifecycleStore<'_>,
) -> (String, String, String, String, String) {
    let (feature_uid, task_uid, child_uid, _) = support::create_feature_graph(lifecycle);
    let (task, _) = support::finish_task_through_qa(fixture, lifecycle, &task_uid);
    let work_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let completion_event_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let receipt = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: task_uid.clone(),
        subject_sha256: task.implementation_sha256.clone().unwrap(),
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 10,
        evidence_refs: vec!["spec/reconciled".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    lifecycle
        .apply_task_event(
            &task_uid,
            task.revision,
            &completion_event_uid,
            TaskEvent::ReconcileSpec {
                receipt,
                pr_work_item_uid: work_uid.clone(),
            },
        )
        .unwrap();
    (
        feature_uid,
        task_uid,
        child_uid,
        work_uid,
        completion_event_uid,
    )
}

#[test]
fn replay_does_not_restore_pr_scope_that_omits_a_completed_subtask() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, _task_uid, child_uid, work_uid, completion_event_uid) =
        completed_parent_with_pr(&fixture, &lifecycle);

    let state_dir = fixture.0.join(".koolade-packet/state");
    let event_path = state_dir
        .join("lifecycle_events")
        .join(format!("{completion_event_uid}.json"));
    let mut event: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&event_path).unwrap()).unwrap();
    let included = event["prPreparationAfter"]["includedTaskUids"]
        .as_array_mut()
        .unwrap();
    included.retain(|uid| uid.as_str() != Some(child_uid.as_str()));
    std::fs::write(&event_path, serde_json::to_vec(&event).unwrap()).unwrap();

    let pr_path = state_dir
        .join("lifecycle_pr_work_items")
        .join(format!("{work_uid}.json"));
    std::fs::remove_file(&pr_path).unwrap();
    assert!(lifecycle.replay_feature(&feature_uid).is_err());
    assert!(!pr_path.exists());
}

#[test]
fn replay_rejects_orphan_task_events_before_restoring_task_snapshots() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, task_uid, _, _) = support::create_feature_graph(&lifecycle);
    let task = lifecycle.load_task(&task_uid).unwrap();
    let event_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    lifecycle
        .apply_task_event(&task_uid, task.revision, &event_uid, TaskEvent::Start)
        .unwrap();

    let state_dir = fixture.0.join(".koolade-packet/state");
    let event_path = state_dir
        .join("lifecycle_events")
        .join(format!("{event_uid}.json"));
    let mut event: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&event_path).unwrap()).unwrap();
    event["taskUid"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
    std::fs::write(&event_path, serde_json::to_vec(&event).unwrap()).unwrap();

    let task_path = state_dir
        .join("lifecycle_tasks")
        .join(format!("{task_uid}.json"));
    let feature_path = state_dir
        .join("lifecycle_features")
        .join(format!("{feature_uid}.json"));
    std::fs::remove_file(&feature_path).unwrap();
    std::fs::remove_file(&task_path).unwrap();
    assert!(lifecycle.replay_feature(&feature_uid).is_err());
    assert!(!feature_path.exists());
    assert!(!task_path.exists());
}

#[test]
fn replay_rejects_orphan_pr_events_before_restoring_pr_snapshots() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, task_uid, _, work_uid, _) = completed_parent_with_pr(&fixture, &lifecycle);
    let task = lifecycle.load_task(&task_uid).unwrap();
    let work = lifecycle.load_pr_preparation(&work_uid).unwrap();
    let receipt = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: work_uid.clone(),
        subject_sha256: task.implementation_sha256.clone().unwrap(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 20,
        evidence_refs: vec![format!("user-action:create-pr:{work_uid}")],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let authorization_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    lifecycle
        .authorize_pr_publication(&work_uid, work.revision, &authorization_uid, &receipt)
        .unwrap();

    let state_dir = fixture.0.join(".koolade-packet/state");
    let event_path = state_dir
        .join("lifecycle_events")
        .join(format!("{authorization_uid}.json"));
    let mut event: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&event_path).unwrap()).unwrap();
    event["event"]["event"]["work_item_uid"] =
        serde_json::Value::String(uuid::Uuid::new_v4().to_string());
    std::fs::write(&event_path, serde_json::to_vec(&event).unwrap()).unwrap();

    let pr_path = state_dir
        .join("lifecycle_pr_work_items")
        .join(format!("{work_uid}.json"));
    let feature_path = state_dir
        .join("lifecycle_features")
        .join(format!("{feature_uid}.json"));
    std::fs::remove_file(&feature_path).unwrap();
    std::fs::remove_file(&pr_path).unwrap();
    assert!(lifecycle.replay_feature(&feature_uid).is_err());
    assert!(!feature_path.exists());
    assert!(!pr_path.exists());
}

#[test]
fn replay_does_not_restore_a_done_parent_when_a_subtask_is_unfinished() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, task_uid, child_uid, work_uid, _) =
        completed_parent_with_pr(&fixture, &lifecycle);
    let state_dir = fixture.0.join(".koolade-packet/state");
    let child_completion = lifecycle
        .events_for_feature(&feature_uid)
        .unwrap()
        .into_iter()
        .find(|event| {
            event.task_uid.as_deref() == Some(child_uid.as_str())
                && matches!(
                    &event.event,
                    crate::core::lifecycle::LifecycleEventKind::Task(
                        TaskEvent::SubtaskReconciled { .. }
                    )
                )
        })
        .unwrap();
    std::fs::remove_file(
        state_dir
            .join("lifecycle_events")
            .join(format!("{}.json", child_completion.uid)),
    )
    .unwrap();
    let feature_path = state_dir
        .join("lifecycle_features")
        .join(format!("{feature_uid}.json"));
    let parent_path = state_dir
        .join("lifecycle_tasks")
        .join(format!("{task_uid}.json"));
    let child_path = state_dir
        .join("lifecycle_tasks")
        .join(format!("{child_uid}.json"));
    let pr_path = state_dir
        .join("lifecycle_pr_work_items")
        .join(format!("{work_uid}.json"));
    for path in [&feature_path, &parent_path, &child_path, &pr_path] {
        std::fs::remove_file(path).unwrap();
    }

    assert!(lifecycle.replay_feature(&feature_uid).is_err());
    assert!(
        [&feature_path, &parent_path, &child_path, &pr_path]
            .iter()
            .all(|path| !path.exists())
    );
}
