use super::*;

fn frozen_scope(store: &PlanningStore, work: &PullRequestPreparation) -> PrPublicationScopeV1 {
    let lifecycle = LifecycleStore::new(store);
    let parent = lifecycle.load_task(&work.task_uid).unwrap();
    let task_evidence = work
        .included_task_uids
        .iter()
        .map(|uid| {
            let task = lifecycle.load_task(uid).unwrap();
            PrPublicationTaskEvidenceV1 {
                task_uid: uid.clone(),
                task_revision: task.revision,
                reviewed_head_sha: task.reviewed_code_head_sha.unwrap(),
                reviewed_diff_hash: task.implementation_sha256.unwrap(),
                code_review_receipt: task.code_review.unwrap(),
                qa_receipt: task.runtime_qa.unwrap(),
                spec_sync_receipt: task.spec_reconciliation.unwrap(),
            }
        })
        .collect();
    PrPublicationScopeV1 {
        schema_version: 1,
        publication_generation: 1,
        source_repository_id: "synthetic-repository".into(),
        source_sha: parent.reviewed_code_head_sha.unwrap(),
        implementation_branch: "task/fixture".into(),
        destination_branch: "main".into(),
        expected_base_sha: "d".repeat(40),
        reviewed_diff_hash: parent.implementation_sha256.unwrap(),
        included_task_uids: work.included_task_uids.clone(),
        accepted_spec_revision: lifecycle
            .load_spec_revision(&work.feature_uid)
            .unwrap()
            .unwrap()
            .seq,
        task_evidence,
    }
}

#[test]
// Production boundary: authorization snapshots task gates and publication
// inputs into durable PR work. Reopen proves serialization and replay retain
// the exact scope; synthetic file store, moderate setup, no external services.
fn authorized_publication_scope_is_frozen_and_survives_reopen() {
    let (fixture, store, work, receipt) = super::ready();
    let lifecycle = LifecycleStore::new(&store);
    let scope = frozen_scope(&store, &work);
    let mut forged_qa = scope.clone();
    forged_qa.task_evidence[0].qa_receipt.actor = GateActor::User;
    assert!(forged_qa.validate(&work.included_task_uids).is_err());
    assert!(
        lifecycle
            .authorize_pr_publication_with_scope(
                &work.work_item_uid,
                work.revision,
                &uuid::Uuid::new_v4().to_string(),
                &receipt,
                &forged_qa,
            )
            .is_err()
    );
    let mut stale_task = scope.clone();
    stale_task.task_evidence[0].task_revision += 1;
    assert!(stale_task.validate(&work.included_task_uids).is_ok());
    assert!(
        lifecycle
            .authorize_pr_publication_with_scope(
                &work.work_item_uid,
                work.revision,
                &uuid::Uuid::new_v4().to_string(),
                &receipt,
                &stale_task,
            )
            .is_err()
    );
    let authorized = lifecycle
        .authorize_pr_publication_with_scope(
            &work.work_item_uid,
            work.revision,
            &uuid::Uuid::new_v4().to_string(),
            &receipt,
            &scope,
        )
        .unwrap();
    assert_eq!(authorized.publication_scope.as_ref(), Some(&scope));
    assert_eq!(authorized.publication_lease_epoch(), 1);
    let reopened_store = fixture.store();
    let reopened = LifecycleStore::new(&reopened_store);
    assert_eq!(
        reopened.load_pr_preparation(&work.work_item_uid).unwrap(),
        authorized
    );

    let mut changed_scope = scope;
    changed_scope.expected_base_sha = "e".repeat(40);
    assert!(
        reopened
            .authorize_pr_publication_with_scope(
                &work.work_item_uid,
                authorized.revision,
                &uuid::Uuid::new_v4().to_string(),
                &receipt,
                &changed_scope,
            )
            .is_err()
    );
}

#[test]
fn retry_cannot_backfill_a_scope_for_legacy_authorization() {
    let (_fixture, store, work, receipt) = super::ready();
    let lifecycle = LifecycleStore::new(&store);
    let authorized = lifecycle
        .authorize_pr_publication(
            &work.work_item_uid,
            work.revision,
            &uuid::Uuid::new_v4().to_string(),
            &receipt,
        )
        .unwrap();
    let failure = lifecycle
        .record_pr_publication_failure(
            &work.work_item_uid,
            authorized.revision,
            &uuid::Uuid::new_v4().to_string(),
            Some(authorized.publication_lease_epoch()),
            "synthetic timeout",
        )
        .unwrap();
    let scope = frozen_scope(&store, &work);
    assert!(
        lifecycle
            .authorize_pr_publication_with_scope(
                &work.work_item_uid,
                failure.revision,
                &uuid::Uuid::new_v4().to_string(),
                &receipt,
                &scope,
            )
            .is_err()
    );
}

#[test]
// Production boundary: durable history must retain an old explicit
// authorization as audit evidence while refusing to bind it to a refreshed
// destination head. This crosses task replay and PR projection replay in a
// local file store; a unit test cannot prove the two event streams replay
// together. It uses a temporary file store and synthetic receipts, with no
// provider, Docker, or network setup.
fn destination_head_invalidation_keeps_prior_authorization_historical() {
    let (_fixture, store, work, receipt) = super::ready();
    let lifecycle = LifecycleStore::new(&store);
    let scope = frozen_scope(&store, &work);
    let authorized = lifecycle
        .authorize_pr_publication_with_scope(
            &work.work_item_uid,
            work.revision,
            &uuid::Uuid::new_v4().to_string(),
            &receipt,
            &scope,
        )
        .unwrap();
    let attention = lifecycle
        .record_pr_publication_failure(
            &work.work_item_uid,
            authorized.revision,
            &uuid::Uuid::new_v4().to_string(),
            Some(authorized.publication_lease_epoch()),
            "destination branch changed before push",
        )
        .unwrap();
    let task = lifecycle.load_task(&work.task_uid).unwrap();
    lifecycle
        .apply_task_event(
            &task.task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::PublicationHeadChanged {
                code_sha256: "e".repeat(64),
                head_sha: "b".repeat(40),
            },
        )
        .unwrap();

    let replayed = lifecycle.replay_feature(&work.feature_uid).unwrap();
    assert_eq!(replayed.feature_uid, work.feature_uid);
    let refreshed_task = lifecycle.load_task(&work.task_uid).unwrap();
    let current_work = lifecycle.load_pr_preparation(&work.work_item_uid).unwrap();
    assert_eq!(
        refreshed_task.phase,
        crate::core::lifecycle::TaskPhase::ReReview
    );
    assert_eq!(current_work.revision, attention.revision);
    assert_eq!(
        current_work.status,
        PullRequestPreparationStatus::NeedsAttention
    );
    assert_eq!(
        current_work.publication_authorization,
        authorized.publication_authorization
    );
    assert_eq!(current_work.publication_scope, authorized.publication_scope);
}

#[path = "scope/leases.rs"]
mod leases;

#[path = "scope/stale_authorization.rs"]
mod stale_authorization;
