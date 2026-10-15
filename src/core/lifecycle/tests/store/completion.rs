#[path = "completion/replay_integrity.rs"]
mod replay_integrity;
#[path = "completion/support.rs"]
mod support;

use super::Fixture;
use crate::core::lifecycle::{
    FeatureEvent, GateActor, GateDecision, GateReceipt, LifecycleStore,
    PullRequestPreparationStatus, TaskEvent, TaskPhase,
};

#[test]
fn task_completion_and_pr_preparation_are_committed_as_one_transition() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, task_uid, child_uid, _feature_revision_before_task_creation) =
        support::create_feature_graph(&lifecycle);
    let (mut task, _request) = support::finish_task_through_qa(&fixture, &lifecycle, &task_uid);
    let work_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let spec = GateReceipt {
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
    task = lifecycle
        .apply_task_event(
            &task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::ReconcileSpec {
                receipt: spec,
                pr_work_item_uid: work_uid.clone(),
            },
        )
        .unwrap();
    assert_eq!(task.phase, TaskPhase::Done);
    let mut work = lifecycle.load_pr_preparation(&work_uid).unwrap();
    assert_eq!(work.task_uid, task_uid);
    assert_eq!(work.feature_uid, feature_uid);
    assert_eq!(
        work.included_task_uids,
        [task_uid.clone(), child_uid.clone()]
    );
    assert_eq!(
        work.status,
        PullRequestPreparationStatus::AwaitingAuthorization
    );
    assert_eq!(work.publication_authorization, None);
    let authorization = GateReceipt {
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
    let mut forged = authorization.clone();
    forged.subject_sha256 = "f".repeat(64);
    assert!(
        lifecycle
            .authorize_pr_publication(
                &work_uid,
                work.revision,
                &uuid::Uuid::new_v4().to_string(),
                &forged,
            )
            .is_err()
    );
    let authorization_event = uuid::Uuid::new_v4().to_string();
    let publishing = lifecycle
        .authorize_pr_publication(
            &work_uid,
            work.revision,
            &authorization_event,
            &authorization,
        )
        .unwrap();
    assert_eq!(publishing.status, PullRequestPreparationStatus::Publishing);
    assert!(
        lifecycle
            .authorize_pr_publication(
                &work_uid,
                work.revision,
                &uuid::Uuid::new_v4().to_string(),
                &authorization,
            )
            .is_ok()
    );
    let opened = lifecycle
        .record_pr_publication_state(
            &work_uid,
            publishing.revision,
            &uuid::Uuid::new_v4().to_string(),
            crate::core::lifecycle::PullRequestPublicationState {
                status: PullRequestPreparationStatus::Opened,
                provider_pr_id: Some(42),
                provider_pr_url: Some("https://github.com/example/project/pull/42".into()),
                head_sha: Some("a".repeat(40)),
                base_sha: None,
            },
        )
        .unwrap();
    assert_eq!(opened.status, PullRequestPreparationStatus::Opened);
    work = opened.clone();
    assert!(
        lifecycle
            .record_pr_publication_state(
                &work_uid,
                publishing.revision,
                &uuid::Uuid::new_v4().to_string(),
                crate::core::lifecycle::PullRequestPublicationState {
                    status: PullRequestPreparationStatus::Merged,
                    provider_pr_id: Some(42),
                    provider_pr_url: Some("https://github.com/example/project/pull/42".into()),
                    head_sha: Some("a".repeat(40)),
                    base_sha: None,
                },
            )
            .is_err()
    );
    let reopened_store_after_publish = fixture.store();
    let reopened_after_publish = LifecycleStore::new(&reopened_store_after_publish);
    assert_eq!(
        reopened_after_publish
            .load_pr_preparation(&work_uid)
            .unwrap(),
        opened,
        "publication authorization and opened status survive restart"
    );
    assert!(
        lifecycle
            .apply_task_event(
                &task_uid,
                task.revision - 1,
                &uuid::Uuid::new_v4().to_string(),
                TaskEvent::ReconcileSpec {
                    receipt: task.spec_reconciliation.clone().unwrap(),
                    pr_work_item_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
                },
            )
            .is_err()
    );
    let reopened_store = fixture.store();
    let reopened = LifecycleStore::new(&reopened_store);
    assert_eq!(
        reopened.load_pr_preparation(&work_uid).unwrap(),
        work,
        "the PR work item and its publication state survive restart"
    );
    let expected_parent = reopened.load_task(&task_uid).unwrap();
    let expected_child = reopened.load_task(&child_uid).unwrap();
    let state_dir = fixture.0.join(".koolade-packet/state");
    std::fs::remove_file(
        state_dir
            .join("lifecycle_tasks")
            .join(format!("{task_uid}.json")),
    )
    .unwrap();
    std::fs::remove_file(
        state_dir
            .join("lifecycle_tasks")
            .join(format!("{child_uid}.json")),
    )
    .unwrap();
    std::fs::remove_file(
        state_dir
            .join("lifecycle_pr_work_items")
            .join(format!("{work_uid}.json")),
    )
    .unwrap();
    assert_eq!(
        reopened.replay_feature(&feature_uid).unwrap(),
        reopened.load_feature(&feature_uid).unwrap()
    );
    assert_eq!(reopened.load_task(&task_uid).unwrap(), expected_parent);
    assert_eq!(reopened.load_task(&child_uid).unwrap(), expected_child);
    assert_eq!(reopened.load_pr_preparation(&work_uid).unwrap(), work);
    assert_eq!(
        reopened_store
            .list_record_paths("state/lifecycle_pr_work_items")
            .unwrap()
            .len(),
        1,
        "replaying completion cannot create a second PR preparation item"
    );
    let feature = lifecycle.load_feature(&feature_uid).unwrap();
    let complete = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::TasksCompleted,
        )
        .unwrap();
    assert_eq!(
        complete.phase,
        crate::core::lifecycle::FeaturePhase::Complete
    );

    let authorization_path = state_dir
        .join("lifecycle_events")
        .join(format!("{authorization_event}.json"));
    let pr_snapshot_path = state_dir
        .join("lifecycle_pr_work_items")
        .join(format!("{work_uid}.json"));
    let mut authorization_record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&authorization_path).unwrap()).unwrap();
    authorization_record["prPreparationAfter"]["publication_authorization"]["subject_sha256"] =
        "f".repeat(64).into();
    std::fs::write(
        &authorization_path,
        serde_json::to_vec(&authorization_record).unwrap(),
    )
    .unwrap();
    std::fs::remove_file(&pr_snapshot_path).unwrap();
    assert!(reopened.replay_feature(&feature_uid).is_err());
    assert!(!pr_snapshot_path.exists());
}

#[test]
fn replay_does_not_restore_parent_without_subtask_projection() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, task_uid, _child_uid, _) = support::create_feature_graph(&lifecycle);
    let parent_path = fixture.0.join(format!(
        ".koolade-packet/state/lifecycle_tasks/{task_uid}.json"
    ));
    let subtask_event = lifecycle
        .events_for_feature(&feature_uid)
        .unwrap()
        .into_iter()
        .find(|event| {
            matches!(
                &event.event,
                crate::core::lifecycle::LifecycleEventKind::SubtaskCreated { parent_task_uid }
                    if parent_task_uid == &task_uid
            )
        })
        .unwrap();
    let event_path = fixture.0.join(format!(
        ".koolade-packet/state/lifecycle_events/{}.json",
        subtask_event.uid
    ));
    let mut event: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&event_path).unwrap()).unwrap();
    event.as_object_mut().unwrap().remove("parentTaskAfter");
    std::fs::write(&event_path, serde_json::to_vec(&event).unwrap()).unwrap();
    std::fs::remove_file(&parent_path).unwrap();

    assert!(lifecycle.replay_feature(&feature_uid).is_err());
    assert!(!parent_path.exists());
}
