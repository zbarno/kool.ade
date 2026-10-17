use super::*;

#[test]
// Production boundary: sequential user commands commit canonical prose and
// immutable authorizations through PlanningStore. Validator tests cannot prove
// the second command preserves the first decision. Local app/Git fixture only;
// moderate setup, no external services.
fn multiple_proposals_can_be_adopted_or_declined_without_losing_prior_consent() {
    for choices in [[false, false], [true, false], [false, true], [true, true]] {
        let mut app = crate::app::root::board_tests::fixture();
        let proposal = seed_proposal(&mut app);
        let Screen::Connected(project) = &mut app.screen else {
            panic!("connected")
        };
        let lifecycle = LifecycleStore::new(&project.state.planning_store);
        let (current, mut deltas) = lifecycle
            .spec_revision_history(&proposal.feature_uid)
            .unwrap()
            .pop()
            .unwrap();
        let mut second = deltas[0].clone();
        second.requirement_id = "REQ-002".into();
        second.old_target = Some("REQ-002: Secondary target wording.".into());
        second.proposed_target = Some("REQ-002: Adopted secondary target wording.".into());
        deltas.push(second);
        lifecycle
            .append_spec_revision(
                &proposal.feature_uid,
                current.seq,
                SpecRevisionDraftV1 {
                    content_hash: current.content_hash,
                    source_revision: current.source_revision,
                    produced_by_run_id: None,
                    origin_events: current.origin_events,
                    decision_refs: current.decision_refs,
                    delta_approval_refs: Vec::new(),
                    approved_target_hash: current.approved_target_hash,
                    implementation_head_sha: None,
                    reconciliation_status: SpecReconciliationStatusV1::NeedsHumanDecision,
                    outcome_reason: Some("Two proposals require explicit decisions.".into()),
                    deltas,
                },
            )
            .unwrap();
        for (index, adopt) in choices.into_iter().enumerate() {
            decide_feature_delta(
                project,
                &proposal.feature_uid,
                if index == 0 { "REQ-001" } else { "REQ-002" },
                2 + index as u64,
                adopt,
            )
            .unwrap();
        }
        let lifecycle = LifecycleStore::new(&project.state.planning_store);
        let (current, deltas) = lifecycle
            .spec_revision_history(&proposal.feature_uid)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(current.seq, 4);
        assert_eq!(
            current.reconciliation_status,
            SpecReconciliationStatusV1::Synchronized
        );
        for delta in &deltas {
            let decision = delta.operator_decision.as_ref().unwrap();
            assert!(current.decision_refs.contains(&decision.receipt_uid));
            let authorization = lifecycle
                .load_feature_delta_authorization(&decision.receipt_uid)
                .unwrap();
            assert_eq!(
                authorization.resulting_product_sha256,
                decision.resulting_spec_sha256
            );
        }
        assert!(
            lifecycle
                .product_specification_aggregate()
                .unwrap()
                .findings
                .is_empty()
        );
        assert!(
            lifecycle
                .list_spec_sync_queue(&proposal.feature_uid, false)
                .unwrap()
                .iter()
                .all(|entry| !current.origin_events.contains(&entry.uid))
        );
    }
}
