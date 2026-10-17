use super::*;

#[test]
fn joint_batch_origins_reference_the_exact_persisted_delta() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (uid, _) =
        traceability::support::reviewed_feature(&fixture, &store, "CHG-101", "CHG-101-origins");
    let lifecycle = LifecycleStore::new(&store);
    let feature = lifecycle.load_feature(&uid).unwrap();
    let current = lifecycle.load_spec_revision(&uid).unwrap().unwrap();
    let requirement = crate::core::lifecycle::plan_snapshot::load_persisted(
        &store,
        feature.plan_snapshot_uid.as_deref().unwrap(),
    )
    .unwrap()
    .unwrap()
    .requirements[0]
        .stable_id
        .clone();
    let first = lifecycle
        .enqueue_manual_spec_refresh(&uid, "joint-first")
        .unwrap();
    let second = lifecycle
        .enqueue_manual_spec_refresh(&uid, "joint-second")
        .unwrap();
    let mut draft = Fixture::draft(&store, feature.revision, first.uid.clone());
    draft.origin_events.push(second.uid.clone());
    draft.decision_refs = feature.decision_refs;
    draft.approved_target_hash = feature.approved_plan_sha256;
    draft.no_change_reason = None;
    draft.reconciliation_status = SpecReconciliationStatusV1::OutstandingNonblockingFindings;
    draft.deltas.push(SpecDeltaRecordV1 {
        requirement_id: requirement.clone(),
        event_id: second.uid.clone(),
        old_target: Some("Existing documentation".into()),
        proposed_target: Some("Clarified documentation".into()),
        observed_current: None,
        classification: SpecDeltaClassificationV1::Documentation,
        evidence_refs: vec![format!("spec-trigger:{}", second.uid)],
        decision_refs: Vec::new(),
        test_refs: Vec::new(),
        affected_task_ids: Vec::new(),
        needs_user_reapproval: false,
        followup_ids: Vec::new(),
        operator_decision: None,
    });
    let written = lifecycle
        .append_spec_revision(&uid, current.seq, draft)
        .unwrap();
    let outcomes = lifecycle
        .specification_origin_dispositions(&uid, written.seq)
        .unwrap()
        .unwrap();
    assert_eq!(outcomes.len(), 2);
    for outcome in &outcomes {
        assert_eq!(
            outcome.outcome,
            SpecOriginOutcomeV1::DeltaReferences {
                deltas: vec![SpecDeltaReferenceV1 {
                    event_uid: second.uid.clone(),
                    requirement_id: requirement.clone()
                }],
            }
        );
    }
    let (mut history, _) = store
        .read_record::<SpecRevisionHistory>(&history_path(&uid, written.seq))
        .unwrap();
    let reference = match &mut history.origin_dispositions.as_mut().unwrap()[0].outcome {
        SpecOriginOutcomeV1::DeltaReferences { deltas } => &mut deltas[0],
        _ => unreachable!(),
    };
    reference.event_uid = first.uid;
    assert!(
        validation::validate_history(&history, 1).is_err(),
        "references must resolve to the exact delta, not merely a known origin"
    );
}
