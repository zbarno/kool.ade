use super::*;

#[test]
// Production boundary: the same file-backed append API used by the adoption
// action. Mutable delta fields alone must not advance the approved target.
fn direct_store_append_rejects_fabricated_operator_adoption() {
    let mut app = crate::app::root::board_tests::fixture();
    let proposal = seed_proposal(&mut app);
    let Screen::Connected(project) = &app.screen else {
        panic!("connected fixture")
    };
    let store = &project.state.planning_store;
    let lifecycle = LifecycleStore::new(store);
    let (current, mut deltas) = lifecycle
        .spec_revision_history(&proposal.feature_uid)
        .unwrap()
        .pop()
        .unwrap();
    let forged_hash = "f".repeat(64);
    let receipt_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    deltas[0].needs_user_reapproval = false;
    deltas[0].operator_decision = Some(crate::core::lifecycle::SpecDeltaOperatorDecisionV1 {
        receipt_uid: receipt_uid.clone(),
        decision: crate::core::lifecycle::SpecDeltaOperatorChoiceV1::Adopted,
        actor: crate::core::lifecycle::GateActor::User,
        recorded_at_ms: 1,
        resulting_spec_sha256: Some(forged_hash.clone()),
    });
    let feature = lifecycle.load_feature(&proposal.feature_uid).unwrap();
    let mut decision_refs = current.decision_refs.clone();
    decision_refs.push(receipt_uid);
    let draft = SpecRevisionDraftV1 {
        no_change_reason: None,
        content_hash: forged_hash.clone(),
        source_revision: feature.revision,
        produced_by_run_id: None,
        origin_events: current.origin_events.clone(),
        decision_refs,
        delta_approval_refs: Vec::new(),
        approved_target_hash: Some(forged_hash),
        implementation_head_sha: current.implementation_head_sha.clone(),
        reconciliation_status: SpecReconciliationStatusV1::Synchronized,
        outcome_reason: None,
        deltas,
    };
    assert!(
        lifecycle
            .append_spec_revision(&proposal.feature_uid, current.seq, draft)
            .is_err()
    );
    assert_eq!(
        lifecycle
            .load_spec_revision(&proposal.feature_uid)
            .unwrap()
            .unwrap(),
        current
    );
}

#[test]
// Production boundary: PlanningStore's file transaction commits product prose,
// current revision and immutable history under one revision fence. Unit tests
// cannot prove the persisted files roll back together. Uses git-init temp data;
// no external service is required.
fn stale_store_fence_rolls_back_product_prose_and_spec_revision_together() {
    let mut app = crate::app::root::board_tests::fixture();
    let proposal = seed_proposal(&mut app);
    let Screen::Connected(project) = &app.screen else {
        panic!("connected fixture")
    };
    let store = &project.state.planning_store;
    let lifecycle = LifecycleStore::new(store);
    let (current, mut deltas) = lifecycle
        .spec_revision_history(&proposal.feature_uid)
        .unwrap()
        .pop()
        .unwrap();
    let stale_store_revision = store.revision().unwrap();
    let documents = crate::artifacts::product_docs::load_documents(store)
        .unwrap()
        .unwrap();
    let document = documents
        .iter()
        .find(|document| document.content.contains(&proposal.old))
        .unwrap();
    let updated = document.content.replacen(&proposal.old, &proposal.new, 1);
    let product_path = store
        .planning_layout()
        .product_root()
        .join(&document.module.path)
        .strip_prefix(store.planning_layout().root())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let rendered = crate::artifacts::product_docs::render_product_with_updates(
        store,
        &[(format!("product:{}", document.module.id), updated.clone())],
    )
    .unwrap()
    .unwrap();
    let content_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&Some(rendered)).unwrap())
    );
    let receipt_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let delta = &mut deltas[0];
    delta.needs_user_reapproval = false;
    delta.operator_decision = Some(crate::core::lifecycle::SpecDeltaOperatorDecisionV1 {
        receipt_uid: receipt_uid.clone(),
        decision: crate::core::lifecycle::SpecDeltaOperatorChoiceV1::Adopted,
        actor: crate::core::lifecycle::GateActor::User,
        recorded_at_ms: 1,
        resulting_spec_sha256: Some(content_hash.clone()),
    });
    let feature = lifecycle.load_feature(&proposal.feature_uid).unwrap();
    let mut decision_refs = feature.decision_refs;
    decision_refs.push(receipt_uid);
    let authorization = crate::core::lifecycle::FeatureDeltaAuthorizationV1 {
        schema_version: 1,
        receipt_uid: delta
            .operator_decision
            .as_ref()
            .unwrap()
            .receipt_uid
            .clone(),
        feature_uid: proposal.feature_uid.clone(),
        source_spec_sequence: current.seq,
        requirement_id: delta.requirement_id.clone(),
        event_uid: delta.event_id.clone(),
        old_target_sha256: Some(format!(
            "{:x}",
            sha2::Sha256::digest(proposal.old.as_bytes())
        )),
        proposed_target_sha256: Some(format!(
            "{:x}",
            sha2::Sha256::digest(proposal.new.as_bytes())
        )),
        resulting_product_sha256: Some(content_hash.clone()),
        decision: crate::core::lifecycle::SpecDeltaOperatorChoiceV1::Adopted,
        actor: crate::core::lifecycle::GateActor::User,
        recorded_at_ms: 1,
    };
    let draft = SpecRevisionDraftV1 {
        no_change_reason: None,
        content_hash: content_hash.clone(),
        source_revision: current.source_revision,
        produced_by_run_id: None,
        origin_events: current.origin_events.clone(),
        decision_refs,
        delta_approval_refs: Vec::new(),
        approved_target_hash: Some(content_hash),
        implementation_head_sha: current.implementation_head_sha.clone(),
        reconciliation_status: SpecReconciliationStatusV1::Synchronized,
        outcome_reason: None,
        deltas,
    };

    store
        .transaction(
            &[(
                "state/unrelated-fence-change".into(),
                b"advance fence".to_vec(),
            )],
            None,
        )
        .unwrap();
    assert!(matches!(
        lifecycle.append_spec_revision_with_feature_authorization(
            &proposal.feature_uid,
            current.seq,
            draft,
            Some(&stale_store_revision),
            authorization,
            vec![(product_path, updated.into_bytes())],
        ),
        Err(crate::artifacts::planning_store::StoreError::StaleRevision { .. })
    ));

    let after = crate::artifacts::product_docs::render_product(store)
        .unwrap()
        .unwrap();
    assert!(after.contains(&proposal.old));
    assert!(!after.contains(&proposal.new));
    let history = lifecycle
        .spec_revision_history(&proposal.feature_uid)
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].0, current);
}
