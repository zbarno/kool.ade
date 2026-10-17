use super::*;

#[test]
fn successful_empty_result_requires_explicit_bounded_rationale_before_any_write() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (uid, origin) = Fixture::feature(&store);
    let lifecycle = LifecycleStore::new(&store);
    for reason in [
        None,
        Some(" ".into()),
        Some("x".repeat(4097)),
        Some("invalid\0reason".into()),
    ] {
        let mut draft = Fixture::draft(&store, 1, origin.clone());
        draft.no_change_reason = reason;
        assert!(
            matches!(lifecycle.append_spec_revision(&uid, 0, draft), Err(StoreError::MalformedState(message)) if message.contains("explicit reason"))
        );
    }
    assert!(lifecycle.load_spec_revision(&uid).unwrap().is_none());
    assert_eq!(
        lifecycle.list_spec_sync_queue(&uid, false).unwrap().len(),
        1
    );
}

#[test]
fn v2_requires_complete_outcomes_while_legacy_history_remains_uninvented() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (uid, event) = Fixture::feature(&store);
    let lifecycle = LifecycleStore::new(&store);
    lifecycle
        .append_spec_revision(&uid, 0, Fixture::draft(&store, 1, event))
        .unwrap();
    let path = history_path(&uid, 1);
    let (history, _) = store.read_record::<SpecRevisionHistory>(&path).unwrap();
    for variant in 0..4 {
        let mut invalid = history.clone();
        match variant {
            0 => invalid.origin_dispositions = None,
            1 => invalid.origin_dispositions = Some(Vec::new()),
            2 => {
                let duplicate = invalid.origin_dispositions.as_ref().unwrap()[0].clone();
                invalid
                    .origin_dispositions
                    .as_mut()
                    .unwrap()
                    .push(duplicate);
            }
            _ => {
                invalid.origin_dispositions.as_mut().unwrap()[0].origin_event_uid =
                    uuid::Uuid::new_v4().to_string()
            }
        }
        assert!(validation::validate_history(&invalid, 1).is_err());
    }
    let mut legacy = history;
    legacy.schema_version = 1;
    legacy.origin_dispositions = None;
    let legacy_bytes = serde_json::to_vec_pretty(&legacy).unwrap();
    store
        .transaction(&[(path.clone(), legacy_bytes.clone())], None)
        .unwrap();
    assert_eq!(
        lifecycle
            .specification_origin_dispositions(&uid, 1)
            .unwrap(),
        None
    );
    assert_eq!(lifecycle.spec_revision_history(&uid).unwrap().len(), 1);
    let refresh = lifecycle
        .enqueue_manual_spec_refresh(&uid, "after-legacy")
        .unwrap();
    lifecycle
        .append_spec_revision(&uid, 1, Fixture::draft(&store, 1, refresh.uid))
        .unwrap();
    assert!(
        lifecycle
            .specification_origin_dispositions(&uid, 2)
            .unwrap()
            .is_some()
    );
    assert_eq!(store.read(&path).unwrap(), legacy_bytes);
}

#[test]
fn unresolved_result_retains_its_reason_without_claiming_no_change() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (uid, event) = Fixture::feature(&store);
    let lifecycle = LifecycleStore::new(&store);
    let mut draft = Fixture::draft(&store, 1, event);
    draft.no_change_reason = None;
    draft.reconciliation_status = SpecReconciliationStatusV1::NeedsHumanDecision;
    draft.outcome_reason =
        Some("The reconciler could not establish the proposed current state.".into());
    lifecycle
        .append_spec_revision(&uid, 0, draft.clone())
        .unwrap();
    assert_eq!(
        lifecycle
            .specification_origin_dispositions(&uid, 1)
            .unwrap()
            .unwrap()[0]
            .outcome,
        SpecOriginOutcomeV1::Unresolved {
            reason: draft.outcome_reason.unwrap()
        }
    );
    assert_eq!(
        lifecycle.list_spec_sync_queue(&uid, false).unwrap().len(),
        1
    );
}
