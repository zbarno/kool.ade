use super::{Fixture, support};
use crate::core::lifecycle::{
    GateActor, GateDecision, GateReceipt, LifecycleStore, TaskEvent, TaskPhase,
};

fn spec_receipt(task_uid: &str, code_hash: String) -> GateReceipt {
    GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: task_uid.to_owned(),
        subject_sha256: code_hash,
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
    }
}

/// Production boundary: the file-backed Done transition rejects unresolved
/// lifecycle triggers before task/PR writes; a task reducer unit test cannot
/// prove transactional queue gating. Synthetic temp store, moderate setup.
#[test]
fn task_done_transition_rejects_pending_spec_triggers() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, task_uid, _, _) = support::create_feature_graph(&lifecycle);
    let (task, _) = support::finish_task_through_qa(&fixture, &lifecycle, &task_uid);
    let manual = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "done-gate-pending")
        .unwrap();
    let error = lifecycle
        .apply_task_event(
            &task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::ReconcileSpec {
                receipt: spec_receipt(&task_uid, task.implementation_sha256.unwrap()),
                pr_work_item_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("unresolved lifecycle trigger"));
    assert_eq!(
        lifecycle.load_task(&task_uid).unwrap().phase,
        TaskPhase::AwaitingSpecReconciliation
    );
    assert!(
        lifecycle
            .list_spec_sync_queue(&feature_uid, false)
            .unwrap()
            .iter()
            .any(|entry| entry.uid == manual.uid)
    );
}

/// A product-document edit after the last durable revision blocks task
/// completion even when its QA and review receipts are still valid.
#[test]
fn task_done_transition_rejects_product_spec_drift() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (_, task_uid, _, _) = support::create_feature_graph(&lifecycle);
    let (task, _) = support::finish_task_through_qa(&fixture, &lifecycle, &task_uid);
    let documents = crate::artifacts::product_docs::load_documents(&store)
        .unwrap()
        .expect("baseline product specification");
    let document = &documents[0];
    let relative_path = store
        .layout()
        .product_root()
        .join(&document.module.path)
        .strip_prefix(store.layout().root())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    store
        .transaction(
            &[(
                relative_path,
                format!("{}\n\nA deliberate product change.\n", document.content).into_bytes(),
            )],
            None,
        )
        .unwrap();
    let error = lifecycle
        .apply_task_event(
            &task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::ReconcileSpec {
                receipt: spec_receipt(&task_uid, task.implementation_sha256.unwrap()),
                pr_work_item_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("drifted"));
    assert_eq!(
        lifecycle.load_task(&task_uid).unwrap().phase,
        TaskPhase::AwaitingSpecReconciliation
    );
}
