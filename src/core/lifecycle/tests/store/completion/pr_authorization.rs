use super::super::Fixture;
use super::{spec_sync, support};
use crate::core::lifecycle::{
    FeatureEvent, GateActor, GateDecision, GateReceipt, LifecycleStore,
    PullRequestPreparationStatus, TaskEvent, TaskPhase,
};
#[path = "pr_authorization/current_support.rs"]
pub(super) mod current_support;

/// Production boundary: file-backed queue/PR/current-evidence transaction;
/// unit tests cannot prove record consistency. Synthetic repo, local Git;
/// moderate setup cost.
#[test]
fn task_completion_and_pr_preparation_are_committed_as_one_transition() {
    let fixture = Fixture::new();
    let (base_commit, pr_head, merge_commit) = current_support::init_merged_repo(&fixture.0);
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, task_uid, child_uid, _feature_revision_before_task_creation) =
        support::create_feature_graph(&lifecycle);
    let (mut task, _request) =
        support::finish_task_through_qa_with_head(&fixture, &lifecycle, &task_uid, pr_head.clone());
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
    let pending = lifecycle
        .list_spec_sync_queue(&feature_uid, false)
        .unwrap()
        .into_iter()
        .map(|entry| entry.source_event_uid)
        .collect::<Vec<_>>();
    spec_sync::append_current_revision(
        &lifecycle,
        &store,
        &feature_uid,
        0,
        pending,
        task.reviewed_code_head_sha.clone(),
    );
    let manual = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "pr-auth-preflight")
        .unwrap();
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
    let pending_error = lifecycle
        .authorize_pr_publication(
            &work_uid,
            work.revision,
            &uuid::Uuid::new_v4().to_string(),
            &authorization,
        )
        .unwrap_err();
    assert!(
        pending_error
            .to_string()
            .contains("pending specification trigger")
    );
    spec_sync::append_current_revision(
        &lifecycle,
        &store,
        &feature_uid,
        1,
        vec![manual.uid],
        task.reviewed_code_head_sha.clone(),
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
            current_support::pr_state(
                PullRequestPreparationStatus::Opened,
                &base_commit,
                &pr_head,
                None,
            ),
        )
        .unwrap();
    assert_eq!(opened.status, PullRequestPreparationStatus::Opened);
    work = lifecycle
        .record_pr_publication_state(
            &work_uid,
            opened.revision,
            &uuid::Uuid::new_v4().to_string(),
            current_support::pr_state(
                PullRequestPreparationStatus::Merged,
                &base_commit,
                &pr_head,
                Some(&merge_commit),
            ),
        )
        .unwrap();
    assert_eq!(
        work.merged_commit_sha.as_deref(),
        Some(merge_commit.as_str())
    );
    current_support::assert_current_evidence(
        &lifecycle,
        &store,
        &task_uid,
        &feature_uid,
        &base_commit,
        &pr_head,
        &merge_commit,
        &fixture.0,
    );
    current_support::assert_merge_trigger(&lifecycle, &feature_uid, &task_uid, work.revision);
    assert!(
        lifecycle
            .spec_reconciliation_candidates()
            .unwrap()
            .iter()
            .any(|candidate| candidate.task_uid == task_uid)
    );
    assert!(
        lifecycle
            .record_pr_publication_state(
                &work_uid,
                publishing.revision,
                &uuid::Uuid::new_v4().to_string(),
                current_support::pr_state(
                    PullRequestPreparationStatus::Merged,
                    &base_commit,
                    &"a".repeat(40),
                    None,
                ),
            )
            .is_err()
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
