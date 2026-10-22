use super::*;

#[test]
// Production boundary: a delayed publisher must not record success or failure
// after retry transfers the lease. A reducer test cannot prove persisted write
// rejection. Durable store, moderate setup, no external service.
fn stale_publication_lease_cannot_complete_or_fail_a_retry() {
    let (_fixture, store, work, receipt) = super::ready();
    let lifecycle = LifecycleStore::new(&store);
    let scope = frozen_scope(&store, &work);
    let first = lifecycle
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
            first.revision,
            &uuid::Uuid::new_v4().to_string(),
            Some(first.publication_lease_epoch()),
            "synthetic timeout",
        )
        .unwrap();
    let retry = lifecycle
        .authorize_pr_publication_with_scope(
            &work.work_item_uid,
            attention.revision,
            &uuid::Uuid::new_v4().to_string(),
            &receipt,
            &scope,
        )
        .unwrap();
    assert_eq!(
        retry.publication_lease_epoch(),
        first.publication_lease_epoch() + 1
    );

    assert!(
        lifecycle
            .record_pr_publication_state(
                &work.work_item_uid,
                retry.revision,
                &uuid::Uuid::new_v4().to_string(),
                PullRequestPublicationState {
                    status: PullRequestPreparationStatus::Opened,
                    provider_pr_id: Some(7),
                    provider_pr_url: Some("https://github.com/example/project/pull/7".into()),
                    head_sha: Some("a".repeat(40)),
                    base_sha: Some("d".repeat(40)),
                    merged_commit_sha: None,
                    lease_epoch: Some(first.publication_lease_epoch()),
                },
            )
            .is_err()
    );
    assert!(
        lifecycle
            .record_pr_publication_failure(
                &work.work_item_uid,
                retry.revision,
                &uuid::Uuid::new_v4().to_string(),
                Some(first.publication_lease_epoch()),
                "late worker failure",
            )
            .is_err()
    );
    assert_eq!(
        lifecycle.load_pr_preparation(&work.work_item_uid).unwrap(),
        retry
    );

    let opened = lifecycle
        .record_pr_publication_state(
            &work.work_item_uid,
            retry.revision,
            &uuid::Uuid::new_v4().to_string(),
            PullRequestPublicationState {
                status: PullRequestPreparationStatus::Opened,
                provider_pr_id: Some(9),
                provider_pr_url: Some("https://github.com/example/project/pull/9".into()),
                head_sha: Some("b".repeat(40)),
                base_sha: Some("d".repeat(40)),
                merged_commit_sha: None,
                lease_epoch: Some(retry.publication_lease_epoch()),
            },
        )
        .unwrap();
    assert!(
        lifecycle
            .record_pr_publication_state(
                &work.work_item_uid,
                opened.revision,
                &uuid::Uuid::new_v4().to_string(),
                PullRequestPublicationState {
                    status: PullRequestPreparationStatus::Opened,
                    provider_pr_id: Some(10),
                    provider_pr_url: Some("https://github.com/example/project/pull/10".into()),
                    head_sha: Some("c".repeat(40)),
                    base_sha: Some("d".repeat(40)),
                    merged_commit_sha: None,
                    lease_epoch: Some(first.publication_lease_epoch()),
                },
            )
            .is_err()
    );
}

#[test]
// The locked transaction fence must reject a concurrent trigger committed
// after scanning, for initial authorization and retry. A local thread and file
// store control the race ordering; moderate setup, no Docker or network.
fn concurrent_trigger_between_scan_and_commit_fences_initial_and_retry_attempts() {
    for retry in 0..3 {
        let (_fixture, store, mut work, receipt) = super::ready();
        if retry > 0 {
            work = super::authorize(&store, &work, &receipt).unwrap();
        }
        if retry == 2 {
            work = LifecycleStore::new(&store)
                .record_pr_publication_failure(
                    &work.work_item_uid,
                    work.revision,
                    &uuid::Uuid::new_v4().to_string(),
                    Some(work.publication_lease_epoch()),
                    "Provider response was lost; reconcile before retry",
                )
                .unwrap();
        }
        let concurrent_store = store.clone();
        let feature_uid = work.feature_uid.clone();
        let error = crate::core::lifecycle::store::authorize_pr_before_commit_for_test(
            &store,
            &work.work_item_uid,
            work.revision,
            &uuid::Uuid::new_v4().to_string(),
            &receipt,
            None,
            move || {
                std::thread::spawn(move || {
                    LifecycleStore::new(&concurrent_store)
                        .enqueue_manual_spec_refresh(&feature_uid, "concurrent-authorize")
                        .unwrap();
                })
                .join()
                .unwrap();
            },
        )
        .unwrap_err();
        assert!(matches!(error, StoreError::StaleRevision { .. }));
        assert_eq!(
            LifecycleStore::new(&store)
                .load_pr_preparation(&work.work_item_uid)
                .unwrap(),
            work
        );
    }
}
