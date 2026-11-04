use super::*;

#[test]
fn pursuing_koolade_product_persists_a_planning_run_from_the_user_receipt() {
    let mut app = crate::app::root::board_tests::fixture();
    let (planning, opportunity_uid) = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        let opportunity_uid = app_product_proposal(project);
        (project.state.planning_store.clone(), opportunity_uid)
    };
    let opportunities = OpportunityStore::new(&planning);
    let (proposal, record_revision) = opportunities.load(&opportunity_uid).unwrap();
    let decision = OpportunityDecisionV1 {
        uid: uuid::Uuid::new_v4().to_string(),
        opportunity_uid: proposal.opportunity_uid.clone(),
        revision_hash: proposal.revision_hash().unwrap(),
        user_id: "local-controller".into(),
        decision: OpportunityChoice::Pursue,
        occurred_at: "2026-10-13T12:00:00Z".into(),
        snooze_until: None,
        explanation: Some("Synthetic UI action".into()),
    };
    opportunities
        .record_user_decision(record_revision, &decision, "local-controller")
        .unwrap();

    let (feature_id, feature_uid, markdown, _) =
        super::super::super::feature::create_draft_feature(&planning, &proposal).unwrap();
    assert!(markdown.contains(&proposal.target.repository_uid));
    assert!(markdown.contains(&proposal.target.source_sha));
    assert!(!markdown.contains(&proposal.target.canonical_remote));
    assert!(
        crate::artifacts::product_docs::active_features(&planning)
            .iter()
            .any(|(id, content)| id == &feature_id && content == &markdown)
    );
    let run = create_self_improvement_run(&planning, &opportunities, &proposal, &feature_uid)
        .unwrap()
        .unwrap();

    assert_eq!(run.phase, SelfImprovementPhaseV1::Planning);
    assert_eq!(run.feature_uid, feature_uid);
    assert_eq!(run.approved_decision_uid, decision.uid);
    assert_eq!(run.canonical_remote, "https://github.com/zbarno/kool.ade");
    let (linked, _) = opportunities.load(&proposal.opportunity_uid).unwrap();
    assert_eq!(linked.status, OpportunityStatusV1::Planning);
    assert!(linked.related_feature_uids.contains(&run.feature_uid));
    assert_eq!(
        SelfImprovementStore::new(&planning)
            .load(&run.run_uid)
            .unwrap(),
        run
    );
}
