use super::{plan, support::*};
use crate::task_review;
use koolade::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::lifecycle::{GateActor, GateDecision, GateReceipt, LifecycleStore, TaskEvent, TaskPhase},
};

#[test]
fn ambiguous_goal_replays_to_reviewed_dag_and_blocks_without_real_qa() {
    let temp = tempfile::tempdir().unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), temp.path(), StoreMode::ManagedLocal);
    let lifecycle = LifecycleStore::new(&store);
    let feature_uid = id();
    lifecycle
        .create_feature(&new_feature(feature_uid.clone()), &id())
        .unwrap();
    let feature = plan::reviewed_feature_plan(&store, &lifecycle, &feature_uid);
    let approved_hash = feature.approved_plan_sha256.unwrap();
    let (parent_uid, mut parent) =
        task_review::reviewed_task_after_repair(&store, &feature_uid, &approved_hash);
    let reopened_store = PlanningStore::new(store.project_id, store.root.clone(), store.mode);
    let reopened = LifecycleStore::new(&reopened_store);
    // No live browser/desktop evidence exists in this fixture. Persist an
    // explicit blocker and prove restart does not synthesize QA/spec receipts,
    // Done state, or a PR work item.
    parent = reopened
        .apply_task_event(
            &parent_uid,
            parent.revision,
            &id(),
            TaskEvent::RuntimeQaUnavailable {
                reason: "No real browser or desktop QA evidence was produced by this fixture"
                    .into(),
            },
        )
        .unwrap();
    let final_store = PlanningStore::new(store.project_id, store.root.clone(), store.mode);
    let final_lifecycle = LifecycleStore::new(&final_store);
    let persisted = final_lifecycle.load_task(&parent_uid).unwrap();
    assert_eq!(persisted.phase, TaskPhase::AwaitingRuntimeQa);
    assert!(persisted.runtime_qa.is_none());
    assert!(persisted.spec_reconciliation.is_none());
    assert!(persisted.pr_work_item_uid.is_none());
    assert!(
        persisted
            .qa_execution
            .unwrap()
            .blocked_reason
            .unwrap()
            .contains("No real browser")
    );
    let spec_receipt = GateReceipt {
        receipt_uid: id(),
        subject_uid: parent_uid.clone(),
        subject_sha256: "d".repeat(64),
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 3,
        evidence_refs: vec!["fixture:must-not-pass-without-qa".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    assert!(
        final_lifecycle
            .reconcile_specification(&parent_uid, persisted.revision, &id(), spec_receipt, &id())
            .is_err(),
        "spec synchronization must remain gated on real QA"
    );
    assert_eq!(parent.revision, persisted.revision);
}
