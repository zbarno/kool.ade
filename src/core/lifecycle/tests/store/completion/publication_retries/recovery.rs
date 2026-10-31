use super::{authorize, ready};
use crate::core::lifecycle::{LifecycleStore, PullRequestPreparationStatus, TaskPhase};

// Production boundary: file-backed Done/PR events, specification-trigger gates,
// retry transaction fences and replay. Unit reducers cannot prove cross-record
// consistency after failure/restart. Synthetic local store/Git; moderate setup,
// no provider, model or Docker. Provider timeout is represented as a blocker,
// not as proof of what happened remotely.
#[test]
fn publication_failures_preserve_done_and_recover_one_authorized_attempt() {
    for authorized_first in [false, true] {
        let (fixture, store, mut work, receipt) = ready();
        let lifecycle = LifecycleStore::new(&store);
        let original_task = lifecycle.load_task(&work.task_uid).unwrap();
        assert_eq!(original_task.phase, TaskPhase::Done);
        if authorized_first {
            work = authorize(&store, &work, &receipt).unwrap();
        }
        let failure = lifecycle
            .record_pr_publication_failure(
                &work.work_item_uid,
                work.revision,
                &uuid::Uuid::new_v4().to_string(),
                authorized_first.then(|| work.publication_lease_epoch()),
                "Provider response timed out; remote outcome is unknown",
            )
            .unwrap();
        assert_eq!(failure.status, PullRequestPreparationStatus::NeedsAttention);
        assert_eq!(
            failure.publication_authorization,
            work.publication_authorization
        );
        assert_eq!(
            failure.recovery.as_ref().unwrap().attempt,
            u64::from(authorized_first)
        );
        assert!(
            failure
                .recovery
                .as_ref()
                .unwrap()
                .last_failure
                .as_deref()
                .unwrap()
                .contains("outcome is unknown")
        );
        assert_eq!(lifecycle.load_task(&work.task_uid).unwrap(), original_task);
        let repeated = lifecycle
            .record_pr_publication_failure(
                &work.work_item_uid,
                failure.revision,
                &uuid::Uuid::new_v4().to_string(),
                authorized_first.then(|| failure.publication_lease_epoch()),
                "Provider response timed out; remote outcome is unknown",
            )
            .unwrap();
        assert_eq!(repeated, failure);

        // Restart reuses the original explicit receipt, then duplicate clicks
        // reuse that same attempt without another write or attempt increment.
        let reopened = fixture.store();
        if authorized_first {
            let mut replacement = receipt.clone();
            replacement.receipt_uid = uuid::Uuid::new_v4().to_string();
            assert!(authorize(&reopened, &failure, &replacement).is_err());
            assert_eq!(
                lifecycle.load_pr_preparation(&work.work_item_uid).unwrap(),
                failure
            );
        }
        let retry = authorize(&reopened, &failure, &receipt).unwrap();
        assert_eq!(retry.status, PullRequestPreparationStatus::Publishing);
        assert_eq!(
            retry.recovery.as_ref().unwrap().attempt,
            1 + u64::from(authorized_first)
        );
        assert_eq!(authorize(&reopened, &retry, &receipt).unwrap(), retry);
        assert_eq!(retry.publication_authorization.as_ref(), Some(&receipt));
        assert_eq!(lifecycle.load_task(&work.task_uid).unwrap(), original_task);

        // The durable blocker and retry transitions must be replayable; losing
        // the PR projection restores the same job rather than inventing one.
        lifecycle.replay_feature(&work.feature_uid).unwrap();
        let path = fixture.0.join(format!(
            "state/lifecycle_pr_work_items/{}.json",
            work.work_item_uid
        ));
        std::fs::remove_file(path).unwrap();
        lifecycle.replay_feature(&work.feature_uid).unwrap();
        assert_eq!(
            lifecycle.load_pr_preparation(&work.work_item_uid).unwrap(),
            retry
        );

        lifecycle
            .enqueue_manual_spec_refresh(&work.feature_uid, "new-requirements-after-failure")
            .unwrap();
        assert!(authorize(&reopened, &retry, &receipt).is_err());
    }
}
