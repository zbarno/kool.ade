use super::*;

#[test]
fn batch_no_change_dispositions_survive_restart_retry_and_stale_writers() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (uid, event) = Fixture::feature(&store);
    let lifecycle = LifecycleStore::new(&store);
    let refresh = lifecycle
        .enqueue_manual_spec_refresh(&uid, "batch-refresh")
        .unwrap();
    let mut draft = Fixture::draft(&store, 1, event.clone());
    draft.origin_events.push(refresh.uid.clone());
    draft.no_change_reason = Some(
        "The approved target is unchanged; the reviewed batch proposes no specification edits."
            .into(),
    );
    let written = lifecycle
        .append_spec_revision(&uid, 0, draft.clone())
        .unwrap();
    let path = history_path(&uid, 1);
    let bytes = store.read(&path).unwrap();
    let outcomes = lifecycle
        .specification_origin_dispositions(&uid, 1)
        .unwrap()
        .unwrap();
    assert_eq!(outcomes.len(), 2);
    for (outcome, origin) in outcomes.iter().zip(&draft.origin_events) {
        assert_eq!(&outcome.origin_event_uid, origin);
        assert_eq!(
            outcome.outcome,
            SpecOriginOutcomeV1::NoChange {
                reason: draft.no_change_reason.clone().unwrap(),
            }
        );
    }
    assert!(
        lifecycle
            .list_spec_sync_queue(&uid, false)
            .unwrap()
            .is_empty()
    );
    let reopened = PlanningStore::new(store.project_id, store.root.clone(), store.mode);
    let lifecycle = LifecycleStore::new(&reopened);
    assert_eq!(
        lifecycle
            .append_spec_revision(&uid, 0, draft.clone())
            .unwrap(),
        written
    );
    assert_eq!(reopened.read(&path).unwrap(), bytes);
    assert_eq!(
        lifecycle
            .specification_origin_dispositions(&uid, 1)
            .unwrap(),
        Some(outcomes)
    );

    let mut changed_rationale = draft;
    changed_rationale.no_change_reason =
        Some("A different rationale cannot rewrite the first receipt.".into());
    assert!(matches!(
        lifecycle.append_spec_revision(&uid, 0, changed_rationale),
        Err(StoreError::StaleRecordRevision { .. })
    ));
    let pending = lifecycle
        .enqueue_manual_spec_refresh(&uid, "stale-origin")
        .unwrap();
    assert!(matches!(
        lifecycle.append_spec_revision(&uid, 0, Fixture::draft(&reopened, 1, pending.uid.clone())),
        Err(StoreError::StaleRecordRevision { .. })
    ));
    assert_eq!(reopened.read(&path).unwrap(), bytes);
    assert_eq!(
        lifecycle.list_spec_sync_queue(&uid, false).unwrap()[0].uid,
        pending.uid
    );
    assert_eq!(lifecycle.spec_revision_history(&uid).unwrap().len(), 1);
}
