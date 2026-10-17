use super::*;

#[test]
// Production boundary: a feature-only decline may resolve an observation that
// has no prior target text. The persisted authorization must preserve that
// absence instead of inventing an empty-string hash.
fn production_decline_authorization_preserves_missing_prior_target() {
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
    deltas[0].old_target = None;
    let feature = lifecycle.load_feature(&proposal.feature_uid).unwrap();
    lifecycle
        .append_spec_revision(
            &proposal.feature_uid,
            current.seq,
            SpecRevisionDraftV1 {
                no_change_reason: None,
                content_hash: current.content_hash.clone(),
                source_revision: feature.revision,
                produced_by_run_id: None,
                origin_events: current.origin_events.clone(),
                decision_refs: current.decision_refs.clone(),
                delta_approval_refs: Vec::new(),
                approved_target_hash: current.approved_target_hash.clone(),
                implementation_head_sha: current.implementation_head_sha.clone(),
                reconciliation_status: SpecReconciliationStatusV1::NeedsHumanDecision,
                outcome_reason: Some("Missing prior target requires a decision.".into()),
                deltas,
            },
        )
        .unwrap();
    app.dispatch_ui_command(ApplicationCommand::DecideFeatureSpecDelta {
        feature_uid: proposal.feature_uid.clone(),
        requirement_id: proposal.requirement_id,
        expected_sequence: current.seq + 1,
        adopt: false,
    });
    let Screen::Connected(project) = &app.screen else {
        panic!("connected fixture")
    };
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let (revision, deltas) = lifecycle
        .spec_revision_history(&proposal.feature_uid)
        .unwrap()
        .pop()
        .unwrap();
    let decision = deltas[0].operator_decision.as_ref().unwrap();
    let authorization = lifecycle
        .load_feature_delta_authorization(&decision.receipt_uid)
        .unwrap();
    assert_eq!(authorization.old_target_sha256, None);
    assert!(authorization.proposed_target_sha256.is_some());
    assert_eq!(authorization.resulting_product_sha256, None);
    assert_eq!(revision.approved_target_hash, current.approved_target_hash);
}
