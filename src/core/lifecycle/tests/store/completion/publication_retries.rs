use super::{Fixture, done_gates::spec_receipt, spec_sync, support};
use crate::{
    artifacts::planning_store::{PlanningStore, StoreError},
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, LifecycleStore, PrPublicationScopeV1,
        PrPublicationTaskEvidenceV1, PullRequestPreparation, PullRequestPreparationStatus,
        PullRequestPublicationState, TaskEvent,
    },
};

#[path = "publication_retries/recovery.rs"]
mod recovery;
#[path = "publication_retries/scope.rs"]
mod scope;

fn ready() -> (Fixture, PlanningStore, PullRequestPreparation, GateReceipt) {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (_, uid, _, _) = support::create_feature_graph(&lifecycle);
    let (task, _) = support::finish_task_through_qa(&fixture, &lifecycle, &uid);
    let work_uid = uuid::Uuid::new_v4().to_string();
    lifecycle
        .apply_task_event(
            &uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::ReconcileSpec {
                receipt: spec_receipt(&uid, task.implementation_sha256.clone().unwrap()),
                pr_work_item_uid: work_uid.clone(),
            },
        )
        .unwrap();
    let mut receipt = spec_receipt(&work_uid, task.implementation_sha256.unwrap());
    receipt.actor = GateActor::User;
    receipt.decision = GateDecision::Approved;
    receipt.evidence_refs = vec![format!("user-action:create-pr:{work_uid}")];
    let work = lifecycle.load_pr_preparation(&work_uid).unwrap();
    (fixture, store, work, receipt)
}

fn authorize(
    store: &PlanningStore,
    work: &PullRequestPreparation,
    receipt: &GateReceipt,
) -> Result<PullRequestPreparation, StoreError> {
    LifecycleStore::new(store).authorize_pr_publication(
        &work.work_item_uid,
        work.revision,
        &uuid::Uuid::new_v4().to_string(),
        receipt,
    )
}

#[test]
// Production boundary: retry authorization reads durable queue/event/projection
// state and validates it before any remote PR operation. Reducers cannot prove
// the exact event exemption. Synthetic file store, moderate setup; no services.
fn retry_exempts_only_its_original_authorization_and_blocks_later_manual_work() {
    let (_fixture, store, work, receipt) = ready();
    let publishing = authorize(&store, &work, &receipt).unwrap();
    assert_eq!(
        authorize(&store, &publishing, &receipt).unwrap(),
        publishing
    );
    LifecycleStore::new(&store)
        .enqueue_manual_spec_refresh(&work.feature_uid, "after-authorization")
        .unwrap();
    assert!(
        authorize(&store, &publishing, &receipt)
            .unwrap_err()
            .to_string()
            .contains("pending specification trigger")
    );
    assert_eq!(
        LifecycleStore::new(&store)
            .load_pr_preparation(&work.work_item_uid)
            .unwrap(),
        publishing
    );
}

#[test]
// Same durable retry boundary/setup; a second Publishing event cannot borrow
// the first event's exemption just by retaining its receipt.
fn later_publishing_event_with_the_same_receipt_remains_pending() {
    let (_fixture, store, work, receipt) = ready();
    let lifecycle = LifecycleStore::new(&store);
    let publishing = authorize(&store, &work, &receipt).unwrap();
    let update = |status, lease_epoch| PullRequestPublicationState {
        status,
        provider_pr_id: None,
        provider_pr_url: None,
        head_sha: None,
        base_sha: None,
        merged_commit_sha: None,
        lease_epoch,
    };
    let attention = lifecycle
        .record_pr_publication_state(
            &work.work_item_uid,
            publishing.revision,
            &uuid::Uuid::new_v4().to_string(),
            update(
                PullRequestPreparationStatus::NeedsAttention,
                Some(publishing.publication_lease_epoch()),
            ),
        )
        .unwrap();
    let pending = lifecycle
        .list_spec_sync_queue(&work.feature_uid, false)
        .unwrap()
        .into_iter()
        .map(|entry| entry.uid)
        .collect();
    spec_sync::append_current_revision(&lifecycle, &store, &work.feature_uid, 0, pending, None);
    let retried = lifecycle
        .record_pr_publication_state(
            &work.work_item_uid,
            attention.revision,
            &uuid::Uuid::new_v4().to_string(),
            update(PullRequestPreparationStatus::Publishing, None),
        )
        .unwrap();
    assert!(
        authorize(&store, &retried, &receipt)
            .unwrap_err()
            .to_string()
            .contains("pending specification trigger")
    );
}
