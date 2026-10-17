use super::{LifecyclePublicationGate, prepare_lifecycle_publication};
use crate::core::lifecycle::{LifecycleStore, PullRequestPreparationStatus, TaskPhase};

mod fixture;
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
