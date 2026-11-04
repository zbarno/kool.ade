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
fn only_current_history_with_complete_outcomes_is_accepted() {
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
            0 => invalid.origin_dispositions.clear(),
            1 => invalid.schema_version = 1,
            2 => {
                let duplicate = invalid.origin_dispositions[0].clone();
                invalid.origin_dispositions.push(duplicate);
            }
            _ => invalid.origin_dispositions[0].origin_event_uid = uuid::Uuid::new_v4().to_string(),
        }
        assert!(validation::validate_history(&invalid, 1).is_err());
    }

    let mut old_schema = history.clone();
    old_schema.schema_version = 1;
    let old_schema_bytes = serde_json::to_vec_pretty(&old_schema).unwrap();
    store
        .transaction(&[(path.clone(), old_schema_bytes)], None)
        .unwrap();
    assert!(lifecycle.spec_revision_history(&uid).is_err());
    assert!(
        lifecycle
            .specification_origin_dispositions(&uid, 1)
            .is_err()
    );

    let mut missing_outcomes = serde_json::to_value(history).unwrap();
    missing_outcomes
        .as_object_mut()
        .unwrap()
        .remove("originDispositions");
    let missing_bytes = serde_json::to_vec(&missing_outcomes).unwrap();
    store
        .transaction(&[(path.clone(), missing_bytes.clone())], None)
        .unwrap();
    assert!(lifecycle.spec_revision_history(&uid).is_err());
    assert!(
        lifecycle
            .specification_origin_dispositions(&uid, 1)
            .is_err()
    );
    assert_eq!(store.read(&path).unwrap(), missing_bytes);
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
