use super::{Fixture, done_gates::spec_receipt, support};
use crate::core::lifecycle::{GateActor, GateDecision, LifecycleStore, TaskEvent, TaskPhase};

fn extra_task(lifecycle: &LifecycleStore<'_>, parent_uid: &str, subtask: bool) -> String {
    let mut task = lifecycle.load_task(parent_uid).unwrap();
    task.task_uid = uuid::Uuid::new_v4().to_string();
    task.revision = 0;
    task.parent_task_uid = subtask.then(|| parent_uid.to_owned());
    task.subtask_uids.clear();
    let uid = task.task_uid.clone();
    lifecycle
        .create_task(&task, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    uid
}

#[test]
// Production boundary: the file-backed subtask Done and PR authorization gates
// must reject drift before writing. Reducers cannot prove persistence ordering.
// Local synthetic store only, moderate setup; no network or Docker.
fn subtask_done_rejects_pending_feature_requests_and_product_drift() {
    for drift in [false, true] {
        let fixture = Fixture::new();
        let store = fixture.store();
        let lifecycle = LifecycleStore::new(&store);
        let (feature_uid, parent_uid, _, _) = support::create_feature_graph(&lifecycle);
        let uid = extra_task(&lifecycle, &parent_uid, true);
        let (task, _) = support::finish_task_through_qa(&fixture, &lifecycle, &uid);
        if drift {
            edit_product(&store);
        } else {
            lifecycle
                .enqueue_manual_spec_refresh(&feature_uid, "subtask-boundary")
                .unwrap();
        }
        let error = lifecycle
            .apply_task_event(
                &uid,
                task.revision,
                &uuid::Uuid::new_v4().to_string(),
                TaskEvent::SubtaskReconciled {
                    receipt: spec_receipt(&uid, task.implementation_sha256.unwrap()),
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains(if drift {
            "drifted"
        } else {
            "unresolved lifecycle trigger"
        }));
        assert_eq!(
            lifecycle.load_task(&uid).unwrap().phase,
            TaskPhase::AwaitingSpecReconciliation
        );
    }
}

#[test]
// Same production persistence boundary/setup as the subtask regression above.
fn pr_authorization_rechecks_product_content_after_task_done() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (_, uid, _, _) = support::create_feature_graph(&lifecycle);
    let (task, _) = support::finish_task_through_qa(&fixture, &lifecycle, &uid);
    let work_uid = uuid::Uuid::new_v4().to_string();
    let receipt = spec_receipt(&uid, task.implementation_sha256.clone().unwrap());
    lifecycle
        .apply_task_event(
            &uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::ReconcileSpec {
                receipt,
                pr_work_item_uid: work_uid.clone(),
            },
        )
        .unwrap();
    edit_product(&store);
    let mut authorization = spec_receipt(&work_uid, task.implementation_sha256.unwrap());
    authorization.actor = GateActor::User;
    authorization.decision = GateDecision::Approved;
    authorization.evidence_refs = vec![format!("user-action:create-pr:{work_uid}")];
    let before = lifecycle.load_pr_preparation(&work_uid).unwrap();
    let error = lifecycle
        .authorize_pr_publication(
            &work_uid,
            before.revision,
            &uuid::Uuid::new_v4().to_string(),
            &authorization,
        )
        .unwrap_err();
    assert!(error.to_string().contains("drifted"));
    assert_eq!(lifecycle.load_pr_preparation(&work_uid).unwrap(), before);
}

fn edit_product(store: &crate::artifacts::planning_store::PlanningStore) {
    let docs = crate::artifacts::product_docs::load_documents(store)
        .unwrap()
        .unwrap();
    let doc = &docs[0];
    let path = store
        .layout()
        .product_root()
        .join(&doc.module.path)
        .strip_prefix(store.layout().root())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    store
        .transaction(
            &[(
                path,
                format!("{}\nChanged product intent.\n", doc.content).into_bytes(),
            )],
            None,
        )
        .unwrap();
}
