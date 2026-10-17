use super::*;

#[test]
// Production boundary: UI command dispatch, durable revision history, and the
// product aggregate. This proves resolving one trigger preserves another;
// unit validation cannot establish that the transaction and projection agree.
fn resolving_one_finding_keeps_other_open_finding_visible() {
    let mut app = crate::app::root::board_tests::fixture();
    let proposal = seed_proposal(&mut app);
    let Screen::Connected(project) = &app.screen else {
        panic!("connected fixture")
    };
    let store = &project.state.planning_store;
    let lifecycle = LifecycleStore::new(store);
    let history = lifecycle
        .spec_revision_history(&proposal.feature_uid)
        .unwrap();
    let (previous, mut deltas) = history.last().unwrap().clone();
    let second = lifecycle
        .enqueue_manual_spec_refresh(&proposal.feature_uid, "second-open-finding")
        .unwrap();
    let mut other = deltas[0].clone();
    other.requirement_id = "REQ-002".into();
    other.old_target = Some("REQ-002: Secondary target wording.".into());
    other.proposed_target = Some("REQ-002: Proposed secondary target.".into());
    other.event_id = second.uid.clone();
    other.evidence_refs = vec![format!("spec-trigger:{}", second.uid)];
    deltas.push(other);
    lifecycle
        .append_spec_revision(
            &proposal.feature_uid,
            previous.seq,
            SpecRevisionDraftV1 {
                no_change_reason: None,
                content_hash: previous.content_hash,
                source_revision: previous.source_revision,
                produced_by_run_id: None,
                origin_events: previous
                    .origin_events
                    .iter()
                    .cloned()
                    .chain(std::iter::once(second.uid.clone()))
                    .collect(),
                decision_refs: Vec::new(),
                delta_approval_refs: Vec::new(),
                approved_target_hash: previous.approved_target_hash,
                implementation_head_sha: None,
                reconciliation_status: SpecReconciliationStatusV1::NeedsHumanDecision,
                outcome_reason: Some("Two independent findings are open.".into()),
                deltas,
            },
        )
        .unwrap();
    let before = lifecycle.product_specification_aggregate().unwrap();
    assert_eq!(before.findings.len(), 2);

    app.dispatch_ui_command(ApplicationCommand::DecideFeatureSpecDelta {
        feature_uid: proposal.feature_uid.clone(),
        requirement_id: proposal.requirement_id,
        expected_sequence: 2,
        adopt: false,
    });

    let Screen::Connected(project) = &app.screen else {
        panic!("connected fixture")
    };
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let after = lifecycle.product_specification_aggregate().unwrap();
    assert_eq!(after.findings.len(), 1);
    assert_eq!(after.findings[0].event_id, second.uid);
}
