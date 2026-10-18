use super::*;

/// Production boundary: recover a saved Pursue receipt after feature creation
/// committed but before the run write, then restart the app from its workspace
/// before retrying. Unit tests cannot prove the app reloads durable planning
/// state and reuses its persisted feature after restart. Uses headless egui,
/// local Git, and temporary stores; no Docker/network; low setup cost.
#[test]
fn retry_recovers_interrupted_pursue_after_feature_write_before_run_write() {
    let mut app = crate::app::root::board_tests::fixture();
    let (uid, decision_revision) = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        let uid = app_product_proposal(project);
        let opportunities = OpportunityStore::new(&project.state.planning_store);
        let (proposal, revision) = opportunities.load(&uid).unwrap();
        let decision = OpportunityDecisionV1 {
            uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            opportunity_uid: uid.clone(),
            revision_hash: proposal.revision_hash().unwrap(),
            user_id: "Koolade Acceptance".into(),
            decision: OpportunityChoice::Pursue,
            occurred_at: "2026-10-18T12:00:00Z".into(),
            snooze_until: None,
            explanation: Some("Synthetic recovery fixture".into()),
        };
        let recorded = opportunities
            .record_user_decision(revision, &decision, "Koolade Acceptance")
            .unwrap();
        let crate::core::opportunity::OpportunityDecisionResult::Recorded {
            proposal: pursued,
            record_revision,
            ..
        } = recorded
        else {
            panic!("first Pursue receipt should be recorded")
        };

        // Simulate interruption after the feature transaction committed, but
        // before the self-improvement run and status update were persisted.
        let (_, feature_uid, _, _) = super::super::super::feature::create_draft_feature(
            &project.state.planning_store,
            &pursued,
        )
        .unwrap();
        assert!(
            SelfImprovementStore::new(&project.state.planning_store)
                .run_for_feature(&feature_uid)
                .unwrap()
                .is_none()
        );
        (uid, record_revision)
    };

    let mut app = crate::app::root::board_tests::restart_fixture(app);
    app.retry_opportunity_planning(&uid, decision_revision);

    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let opportunities = OpportunityStore::new(&project.state.planning_store);
    let (proposal, _) = opportunities.load(&uid).unwrap();
    assert_eq!(proposal.status, OpportunityStatusV1::Planning);
    assert_eq!(proposal.related_feature_uids.len(), 1);
    let decision = opportunities
        .decision_for_opportunity(&uid)
        .unwrap()
        .expect("recovery retains the original user receipt");
    let run_uid = SelfImprovementStore::run_uid_for_decision(&decision.uid).unwrap();
    let run = SelfImprovementStore::new(&project.state.planning_store)
        .load(&run_uid)
        .unwrap();
    assert_eq!(run.phase, SelfImprovementPhaseV1::Planning);
    assert_eq!(proposal.related_feature_uids, vec![run.feature_uid.clone()]);
    assert!(run.clone_uid.is_none());
    assert!(project.active_implementations.is_empty());
    assert!(project.active_turn.is_none());
}
