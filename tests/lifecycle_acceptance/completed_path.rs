mod qa_report;

use super::{plan, support::*};
use koolade::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, LifecycleStore, PullRequestPreparationStatus,
        SpecReconciliationStatusV1, SpecRevisionDraftV1, TaskEvent, TaskPhase,
    },
    runtime_qa::RuntimeQaStore,
};

#[path = "completed_path/parent_subtasks.rs"]
mod parent_subtasks;

#[test]
fn accepted_qa_reconciles_spec_and_creates_one_separate_pr_work_item() {
    let temp = tempfile::tempdir().unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), temp.path(), StoreMode::ManagedLocal);
    let lifecycle = LifecycleStore::new(&store);
    bootstrap_product(&store);
    let feature_uid = id();
    lifecycle
        .create_feature(&new_feature(feature_uid.clone()), &id())
        .unwrap();
    let feature = plan::reviewed_feature_plan(&store, &lifecycle, &feature_uid);
    let feature_events = lifecycle.events_for_feature(&feature_uid).unwrap();
    let origin_events = feature_events.into_iter().map(|event| event.uid).collect();
    lifecycle
        .append_spec_revision(
            &feature_uid,
            0,
            SpecRevisionDraftV1 {
                content_hash: product_hash(&store),
                source_revision: feature.revision,
                produced_by_run_id: None,
                origin_events,
                decision_refs: feature.decision_refs.clone(),
                delta_approval_refs: Vec::new(),
                approved_target_hash: feature.approved_plan_sha256.clone(),
                implementation_head_sha: None,
                reconciliation_status: SpecReconciliationStatusV1::Synchronized,
                outcome_reason: None,
                no_change_reason: Some("Synthetic lifecycle acceptance review found no specification delta for the approved task graph.".into()),
                deltas: Vec::new(),
            },
        )
        .unwrap();
    let plan_hash = feature.approved_plan_sha256.unwrap();
    let task_uid = id();
    let child_uid = id();
    lifecycle
        .create_task(&task(&feature_uid, &task_uid, &plan_hash, None), &id())
        .unwrap();
    lifecycle
        .create_task(
            &task(&feature_uid, &child_uid, &plan_hash, Some(task_uid.clone())),
            &id(),
        )
        .unwrap();
    parent_subtasks::complete_subtask(temp.path(), &store, &lifecycle, &feature_uid, &child_uid);

    let mut task_record = lifecycle.load_task(&task_uid).unwrap();
    task_record = lifecycle
        .apply_task_event(&task_uid, task_record.revision, &id(), TaskEvent::Start)
        .unwrap();
    let implementation_hash = "d".repeat(64);
    task_record = lifecycle
        .apply_task_event(
            &task_uid,
            task_record.revision,
            &id(),
            TaskEvent::ImplementationFinished {
                code_sha256: implementation_hash.clone(),
            },
        )
        .unwrap();
    let mut code_review = role_receipt(
        koolade::harness::prompt_catalog::PromptRole::CodeReviewer,
        GateDecision::Approved,
        &task_uid,
        &implementation_hash,
    );
    code_review.evidence_refs = vec!["fixture:accepted-review-contract/v1".into()];
    task_record = lifecycle
        .apply_task_event(
            &task_uid,
            task_record.revision,
            &id(),
            TaskEvent::CodeReview {
                receipt: code_review,
                reviewed_head_sha: "e".repeat(64),
                review_subject_sha256: "f".repeat(64),
            },
        )
        .unwrap();

    // The persisted report exercises the production validator and lifecycle
    // receipt binding. Its evidence fixture represents an accepted contract;
    // this test does not run interactive browser QA.
    let (qa_request, qa_receipt) =
        qa_report::save_accepted_evidence_fixture(temp.path(), &task_record);
    task_record = lifecycle
        .apply_task_event(
            &task_uid,
            task_record.revision,
            &id(),
            TaskEvent::RuntimeQaRequested(qa_request.clone()),
        )
        .unwrap();
    let qa_store = RuntimeQaStore::new(temp.path().join("qa-records"));
    task_record = lifecycle
        .record_runtime_qa_report(
            &task_uid,
            task_record.revision,
            &id(),
            &qa_store,
            Some(qa_receipt),
        )
        .unwrap();
    assert_eq!(task_record.phase, TaskPhase::AwaitingSpecReconciliation);

    let reopened_store = PlanningStore::new(store.project_id, store.root.clone(), store.mode);
    let reopened = LifecycleStore::new(&reopened_store);
    task_record = reopened.load_task(&task_uid).unwrap();
    assert_eq!(
        task_record.qa_execution.as_ref().unwrap().request.as_ref(),
        Some(&qa_request)
    );
    assert!(task_record.runtime_qa.is_some());

    let work_uid = id();
    parent_subtasks::reconcile_no_change(
        &reopened_store,
        &reopened,
        &feature_uid,
        "Synthetic reopened lifecycle review found no specification delta for the persisted task evidence.",
    );
    let spec_receipt = GateReceipt {
        receipt_uid: id(),
        subject_uid: task_uid.clone(),
        subject_sha256: implementation_hash,
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 4,
        evidence_refs: vec!["fixture:accepted-spec-reconciliation/v1".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let expected_revision = task_record.revision;
    let done = reopened
        .reconcile_specification(
            &task_uid,
            expected_revision,
            &id(),
            spec_receipt.clone(),
            &work_uid,
        )
        .unwrap();
    assert_eq!(done.phase, TaskPhase::Done);
    assert_eq!(done.pr_work_item_uid.as_deref(), Some(work_uid.as_str()));

    let work = reopened.load_pr_preparation(&work_uid).unwrap();
    assert_eq!(work.task_uid, task_uid);
    assert_eq!(
        work.included_task_uids,
        [task_uid.clone(), child_uid.clone()]
    );
    assert_eq!(
        work.status,
        PullRequestPreparationStatus::AwaitingAuthorization
    );
    assert!(work.publication_authorization.is_none());
    assert_eq!(
        reopened.load_task(&done.task_uid).unwrap(),
        done,
        "Done and its PR work item survive app restart"
    );
    assert_eq!(
        reopened_store
            .list_record_paths("state/lifecycle_pr_work_items")
            .unwrap()
            .len(),
        1
    );
    assert!(
        reopened
            .load_task(&child_uid)
            .unwrap()
            .pr_work_item_uid
            .is_none()
    );
    assert!(
        reopened
            .reconcile_specification(&task_uid, expected_revision, &id(), spec_receipt, &work_uid)
            .is_err(),
        "stale replay must not create another PR work item"
    );
    assert_eq!(
        reopened_store
            .list_record_paths("state/lifecycle_pr_work_items")
            .unwrap()
            .len(),
        1
    );
}
