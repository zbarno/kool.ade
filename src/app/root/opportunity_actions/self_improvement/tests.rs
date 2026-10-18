use super::*;
use crate::{
    artifacts::planning_store::StoreMode,
    core::opportunity::{
        EvidenceConfidentialityV1, OpportunityEvidenceRefV1, OpportunityKindV1,
        OpportunityOriginV1, OpportunitySaveResult, OpportunityScopeV1, OpportunityStatusV1,
        OpportunityTargetV1,
    },
    harness::prompt_catalog::{OpportunityChoice, OpportunityDecisionV1},
};

#[test]
fn pursuing_koolade_product_persists_a_planning_run_from_the_user_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.path().join("planning"),
        StoreMode::ManagedLocal,
    );
    crate::artifacts::migration::bootstrap_product_with_store(
        temp.path(),
        &planning,
        "Synthetic Kool.ad/e fixture",
    )
    .unwrap();
    let opportunities = OpportunityStore::new(&planning);
    let proposal = crate::core::opportunity::OpportunityV1 {
        schema_version: 1,
        opportunity_uid: uuid::Uuid::new_v4().to_string(),
        workspace_uid: uuid::Uuid::new_v4().to_string(),
        origin: OpportunityOriginV1::Roadmap,
        kind: OpportunityKindV1::Quality,
        target: OpportunityTargetV1 {
            kind: OpportunityTargetKindV1::KooladeProduct,
            repository_uid: uuid::Uuid::new_v4().to_string(),
            canonical_remote: "https://github.com/zbarno/kool.ade".into(),
            source_sha: "a".repeat(40),
        },
        related_feature_uids: Vec::new(),
        related_task_uids: Vec::new(),
        discovered_at_ms: 1,
        observation_revision: 1,
        evidence_refs: vec![OpportunityEvidenceRefV1 {
            evidence_uid: uuid::Uuid::new_v4().to_string(),
            sha256: "b".repeat(64),
            confidentiality: EvidenceConfidentialityV1::Public,
        }],
        problem_or_opportunity: "Improve planning reliability".into(),
        user_value: "Plans stay consistent".into(),
        expected_outcome: "A reviewed feature is available".into(),
        possible_approaches: vec!["Use current planning records".into()],
        recommended_approach: "Use current planning records".into(),
        alternatives: vec!["Defer".into()],
        estimated_scope: OpportunityScopeV1::Small,
        impact: "Clearer planning".into(),
        cost_or_risk: "Ordinary product risk".into(),
        affected_systems: vec!["planning".into()],
        current_capability_state: "Not implemented".into(),
        generic_workaround: None,
        proposed_approach: "Create a reviewed feature".into(),
        dedup_fingerprint: "c".repeat(64),
        status: OpportunityStatusV1::Proposed,
        revision: 1,
    };
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = opportunities.save_advisory(proposal).unwrap()
    else {
        panic!("test proposal should be created")
    };
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
        super::super::feature::create_draft_feature(&planning, &proposal).unwrap();
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
