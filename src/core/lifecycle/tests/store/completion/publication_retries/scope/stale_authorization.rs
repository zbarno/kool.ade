use super::*;

#[test]
// Production boundary: a Publishing retry must revalidate the persisted scope
// against the accepted specification after request construction. Store tests
// cannot prove this caller snapshot/store-read race; file store only, no
// services, with low setup cost: one temporary lifecycle file-store fixture.
fn publishing_retry_rejects_a_scope_after_the_accepted_spec_revision_changes() {
    let (_fixture, store, work, receipt) = super::super::ready();
    let lifecycle = LifecycleStore::new(&store);
    let scope = super::frozen_scope(&store, &work);
    let publishing = lifecycle
        .authorize_pr_publication_with_scope(
            &work.work_item_uid,
            work.revision,
            &uuid::Uuid::new_v4().to_string(),
            &receipt,
            &scope,
        )
        .unwrap();

    super::spec_sync::append_current_revision(
        &lifecycle,
        &store,
        &work.feature_uid,
        0,
        Vec::new(),
        None,
    );
    let current = lifecycle.load_pr_preparation(&work.work_item_uid).unwrap();
    assert_eq!(current.status, PullRequestPreparationStatus::Publishing);
    assert_eq!(current.revision, publishing.revision);
    assert_eq!(
        lifecycle.load_task(&work.task_uid).unwrap().phase,
        crate::core::lifecycle::TaskPhase::Done
    );
    assert!(
        lifecycle
            .authorize_pr_publication_with_scope(
                &work.work_item_uid,
                current.revision,
                &uuid::Uuid::new_v4().to_string(),
                &receipt,
                &scope,
            )
            .is_err()
    );
    assert_eq!(
        lifecycle.load_pr_preparation(&work.work_item_uid).unwrap(),
        current
    );
}

#[test]
// Production boundary: the authorization API must require the exact frozen
// scope on every Publishing retry. A reducer-only check cannot prove this API
// contract; one temporary file-store fixture, no external service, low setup.
fn publishing_retry_cannot_omit_its_frozen_scope() {
    let (_fixture, store, work, receipt) = super::super::ready();
    let lifecycle = LifecycleStore::new(&store);
    let scope = super::frozen_scope(&store, &work);
    let publishing = lifecycle
        .authorize_pr_publication_with_scope(
            &work.work_item_uid,
            work.revision,
            &uuid::Uuid::new_v4().to_string(),
            &receipt,
            &scope,
        )
        .unwrap();

    assert!(
        lifecycle
            .authorize_pr_publication(
                &work.work_item_uid,
                publishing.revision,
                &uuid::Uuid::new_v4().to_string(),
                &receipt,
            )
            .is_err()
    );
    assert_eq!(
        lifecycle.load_pr_preparation(&work.work_item_uid).unwrap(),
        publishing
    );
}

#[test]
// Replay is the durable recovery boundary for publication authorization.
// A unit validator cannot prove event projections are rejected before snapshot
// restoration; moderate setup: one fixture and one tampered event record, with
// no Docker or network dependency.
fn replay_rejects_a_source_sha_without_a_matching_reviewed_head() {
    let (fixture, store, work, receipt) = super::super::ready();
    let lifecycle = LifecycleStore::new(&store);
    let scope = super::frozen_scope(&store, &work);
    let authorized = lifecycle
        .authorize_pr_publication_with_scope(
            &work.work_item_uid,
            work.revision,
            &uuid::Uuid::new_v4().to_string(),
            &receipt,
            &scope,
        )
        .unwrap();
    let authorization_event = lifecycle
        .events_for_feature(&work.feature_uid)
        .unwrap()
        .into_iter()
        .find(|event| {
            event.pr_preparation_after.as_ref() == Some(&authorized)
                && event
                    .pr_preparation_after
                    .as_ref()
                    .is_some_and(|after| after.publication_authorization.as_ref() == Some(&receipt))
        })
        .unwrap();

    let state_dir = fixture.0.join("state");
    let event_path = state_dir
        .join("lifecycle_events")
        .join(format!("{}.json", authorization_event.uid));
    let mut event: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&event_path).unwrap()).unwrap();
    let tampered_source = if scope.source_sha == "f".repeat(40) {
        "e".repeat(40)
    } else {
        "f".repeat(40)
    };
    event["prPreparationAfter"]["publicationScope"]["sourceSha"] =
        serde_json::Value::String(tampered_source);
    std::fs::write(&event_path, serde_json::to_vec(&event).unwrap()).unwrap();

    let pr_path = state_dir
        .join("lifecycle_pr_work_items")
        .join(format!("{}.json", work.work_item_uid));
    std::fs::remove_file(&pr_path).unwrap();
    assert!(lifecycle.replay_feature(&work.feature_uid).is_err());
    assert!(!pr_path.exists());
}
