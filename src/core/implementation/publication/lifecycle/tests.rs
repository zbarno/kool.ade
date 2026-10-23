use super::{
    LifecyclePublicationGate, finish_lifecycle_publication, prepare_lifecycle_publication,
};
use crate::core::implementation::PullRequestState;
use crate::core::lifecycle::{LifecycleStore, PullRequestPreparationStatus, TaskEvent, TaskPhase};

pub(in crate::core::implementation) mod fixture;
mod recovery;
use fixture::Fixture;

/// Production boundary: the app's publication gate resolves a persisted Done
/// task and writes user authorization through the real PlanningStore. A head
/// comparison unit test cannot prove lookup, authorization ordering, or retry
/// behavior. Uses synthetic accepted gate contracts and local temporary files
/// plus Git; no provider, model, Docker, or browser. Setup cost: one empty Git
/// repository and a small product/lifecycle baseline per OID format.
#[test]
fn integrated_head_cannot_reuse_reviewed_head_authorization() {
    for oid_length in [40, 64] {
        let fixture = Fixture::new(oid_length);
        let lifecycle = LifecycleStore::new(&fixture.store);
        let initial = lifecycle.load_pr_preparation(&fixture.work_uid).unwrap();
        let initial_revision = fixture.store.revision().unwrap();
        let mut implementation = fixture.implementation();

        for head in [None, Some("b".repeat(oid_length))] {
            implementation.verified_head = head;
            assert_head_blocked(&fixture, &implementation);
            assert_eq!(fixture.store.revision().unwrap(), initial_revision);
            assert_eq!(
                lifecycle.load_pr_preparation(&fixture.work_uid).unwrap(),
                initial
            );
        }

        implementation.verified_head = Some(fixture.reviewed_head.clone());
        assert!(matches!(
            prepare_lifecycle_publication(&fixture.store, &implementation).unwrap(),
            LifecyclePublicationGate::Authorized(_)
        ));
        let authorized = lifecycle.load_pr_preparation(&fixture.work_uid).unwrap();
        assert_eq!(authorized.status, PullRequestPreparationStatus::Publishing);
        assert_eq!(
            authorized
                .publication_authorization
                .as_ref()
                .unwrap()
                .subject_sha256,
            fixture.task.implementation_sha256.as_deref().unwrap()
        );

        // A resumed publishing attempt must recheck the current integrated head
        // before reusing the already persisted explicit user authorization.
        let reopened = fixture.reopen();
        implementation.verified_head = Some("c".repeat(oid_length));
        let revision = reopened.revision().unwrap();
        assert!(matches!(
            prepare_lifecycle_publication(&reopened, &implementation).unwrap(),
            LifecyclePublicationGate::Blocked(_)
        ));
        assert_eq!(reopened.revision().unwrap(), revision);
        assert_eq!(
            LifecycleStore::new(&reopened)
                .load_pr_preparation(&fixture.work_uid)
                .unwrap(),
            authorized
        );
        assert_eq!(
            lifecycle.load_task(&fixture.task.task_uid).unwrap(),
            fixture.task
        );
        assert_eq!(fixture.task.phase, TaskPhase::Done);

        implementation.verified_head = Some(fixture.reviewed_head.clone());
        assert!(matches!(
            prepare_lifecycle_publication(&reopened, &implementation).unwrap(),
            LifecyclePublicationGate::Authorized(_)
        ));
        assert_eq!(reopened.revision().unwrap(), revision);
    }
}

#[test]
fn changed_publication_head_reopens_review_and_invalidates_downstream_gates() {
    let fixture = Fixture::new(40);
    let mut task = fixture.task.clone();
    let work_item_uid = task.pr_work_item_uid.clone();
    let review_history = task.review_history.clone();
    let new_hash = "a".repeat(64);
    let new_head = "b".repeat(40);

    task.apply(
        task.revision,
        TaskEvent::PublicationHeadChanged {
            code_sha256: new_hash.clone(),
            head_sha: new_head.clone(),
        },
    )
    .unwrap();

    assert_eq!(task.phase, TaskPhase::ReReview);
    assert_eq!(
        task.implementation_sha256.as_deref(),
        Some(new_hash.as_str())
    );
    assert_eq!(task.reviewed_code_head_sha, None);
    assert_eq!(task.code_review, None);
    assert_eq!(task.qa_execution, None);
    assert_eq!(task.runtime_qa, None);
    assert_eq!(task.spec_reconciliation, None);
    assert_eq!(task.pr_work_item_uid, work_item_uid);
    assert_eq!(task.review_history, review_history);
    task.validate().unwrap();
}

/// Production boundary: replay the durable task and PR projections after a
/// destination update invalidates a previously authorized but unpublished
/// scope. A reducer-only test cannot prove old authorization history remains
/// auditable while no longer authorizing the refreshed head. Uses a local
/// PlanningStore and synthetic accepted gate receipts; no provider, model,
/// Docker, or browser. Setup cost: one temporary repository and lifecycle.
#[test]
fn destination_refresh_keeps_old_authorization_auditable_but_stale() {
    let fixture = Fixture::new(40);
    let lifecycle = LifecycleStore::new(&fixture.store);
    let mut implementation = fixture.implementation();
    implementation.verified_head = Some(fixture.reviewed_head.clone());
    let LifecyclePublicationGate::Authorized(binding) =
        prepare_lifecycle_publication(&fixture.store, &implementation).unwrap()
    else {
        panic!("fixture should authorize publication");
    };
    let work = lifecycle.load_pr_preparation(&fixture.work_uid).unwrap();
    lifecycle
        .record_pr_publication_failure(
            &fixture.work_uid,
            work.revision,
            &uuid::Uuid::new_v4().to_string(),
            Some(binding.lease_epoch),
            "destination branch changed before push",
        )
        .unwrap();
    let task = lifecycle.load_task(&fixture.task.task_uid).unwrap();
    lifecycle
        .apply_task_event(
            &task.task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::PublicationHeadChanged {
                code_sha256: "a".repeat(64),
                head_sha: "b".repeat(40),
            },
        )
        .unwrap();

    let reopened = fixture.reopen();
    let recovered = LifecycleStore::new(&reopened);
    let task = recovered.load_task(&fixture.task.task_uid).unwrap();
    let work = recovered.load_pr_preparation(&fixture.work_uid).unwrap();
    assert_eq!(task.phase, TaskPhase::ReReview);
    assert_eq!(work.status, PullRequestPreparationStatus::NeedsAttention);
    assert!(work.publication_authorization.is_some());
    assert!(work.publication_scope.is_some());
}

#[test]
fn closed_provider_observation_is_durably_recorded_as_closed_unmerged() {
    let fixture = Fixture::new(40);
    let lifecycle = LifecycleStore::new(&fixture.store);
    let mut implementation = fixture.implementation();
    implementation.verified_head = Some(fixture.reviewed_head.clone());
    let LifecyclePublicationGate::Authorized(binding) =
        prepare_lifecycle_publication(&fixture.store, &implementation).unwrap()
    else {
        panic!("fixture should authorize publication");
    };
    implementation.pr_url = Some("https://github.com/fixture/repo/pull/1".into());
    implementation.pr_details = Some(crate::core::implementation::PullRequestDetails {
        number: 1,
        head_sha: fixture.reviewed_head.clone(),
        base_sha: Some("d".repeat(40)),
        base_branch: "main".into(),
        merged_at: None,
    });
    implementation.pr_state = Some(PullRequestState::Closed);

    finish_lifecycle_publication(&fixture.store, &implementation, Some(binding)).unwrap();

    let work = lifecycle.load_pr_preparation(&fixture.work_uid).unwrap();
    assert_eq!(work.status, PullRequestPreparationStatus::ClosedUnmerged);
    assert_eq!(work.provider_pr_id, Some(1));
    assert_eq!(
        work.provider_pr_url.as_deref(),
        Some("https://github.com/fixture/repo/pull/1")
    );
    assert_eq!(
        work.head_sha.as_deref(),
        Some(fixture.reviewed_head.as_str())
    );
    assert_eq!(work.merged_commit_sha, None);
    let reopened_store = fixture.reopen();
    let reopened = LifecycleStore::new(&reopened_store);
    assert_eq!(
        reopened.load_pr_preparation(&fixture.work_uid).unwrap(),
        work
    );
}

fn assert_head_blocked(fixture: &Fixture, implementation: &super::Implementation) {
    let LifecyclePublicationGate::Blocked(reason) =
        prepare_lifecycle_publication(&fixture.store, implementation).unwrap()
    else {
        panic!("unreviewed integrated head must not authorize publication");
    };
    assert!(reason.contains("independently reviewed task head"));
    assert!(reason.contains("runtime QA"));
    assert!(reason.contains("specification reconciliation"));
}
