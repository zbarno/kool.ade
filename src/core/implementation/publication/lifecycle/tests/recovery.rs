use super::super::{
    LifecyclePublication, LifecyclePublicationGate, prepare_lifecycle_publication,
    preserve_done_after_failure,
};
use super::fixture::Fixture;
use crate::core::{
    implementation::ImplementationStatus,
    lifecycle::{LifecycleStore, PullRequestPreparationStatus},
};

// Production boundary: the app's failure classifier writes a durable PR blocker
// and preserves the completed task and its review/QA/spec evidence. Unit status
// mapping cannot prove persisted task isolation. One synthetic file store/Git
// baseline per case; no network/provider/Docker and negligible execution cost.
#[test]
fn publication_error_is_actionable_pr_work_without_interrupting_done_implementation() {
    for authorized_first in [false, true] {
        let fixture = Fixture::new(40);
        let lifecycle = LifecycleStore::new(&fixture.store);
        let mut implementation = fixture.implementation();
        let expected_lease_epoch = if authorized_first {
            assert!(matches!(
                prepare_lifecycle_publication(&fixture.store, &implementation).unwrap(),
                LifecyclePublicationGate::Authorized(LifecyclePublication { lease_epoch: 1, .. })
            ));
            Some(1)
        } else {
            None
        };
        let dir = implementation.task_repository.clone();
        let error = anyhow::anyhow!("Provider request timed out after submission");
        assert!(
            preserve_done_after_failure(
                &fixture.store,
                &dir,
                &mut implementation,
                expected_lease_epoch,
                &error,
            )
            .unwrap()
        );
        assert_eq!(implementation.status, ImplementationStatus::Completed);
        assert!(
            implementation
                .detail
                .contains("remote outcome may be unknown")
        );
        let attention = lifecycle.load_pr_preparation(&fixture.work_uid).unwrap();
        assert_eq!(
            attention.status,
            PullRequestPreparationStatus::NeedsAttention
        );
        assert_eq!(
            lifecycle.load_task(&fixture.task.task_uid).unwrap(),
            fixture.task
        );
        let reopened = fixture.reopen();
        assert!(matches!(
            prepare_lifecycle_publication(&reopened, &implementation).unwrap(),
            LifecyclePublicationGate::Authorized(_)
        ));
        let retry = lifecycle.load_pr_preparation(&fixture.work_uid).unwrap();
        assert_eq!(
            retry.recovery.as_ref().unwrap().attempt,
            1 + u64::from(authorized_first)
        );

        implementation.verified_head = Some("b".repeat(40));
        assert!(matches!(
            prepare_lifecycle_publication(&reopened, &implementation).unwrap(),
            LifecyclePublicationGate::Blocked(_)
        ));
        assert_eq!(
            lifecycle.load_pr_preparation(&fixture.work_uid).unwrap(),
            retry
        );
    }
}
