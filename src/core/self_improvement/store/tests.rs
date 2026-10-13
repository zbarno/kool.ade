use super::*;
use crate::{
    artifacts::planning_store::StoreMode,
    core::opportunity::{
        EvidenceConfidentialityV1, OpportunityEvidenceRefV1, OpportunityKindV1,
        OpportunityOriginV1, OpportunitySaveResult, OpportunityScopeV1, OpportunityTargetV1,
        OpportunityV1,
    },
    harness::prompt_catalog::{OpportunityChoice, OpportunityDecisionV1 as UiDecision},
};

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("self-improvement-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn opportunity() -> OpportunityV1 {
    OpportunityV1 {
        schema_version: 1,
        opportunity_uid: uuid::Uuid::new_v4().to_string(),
        workspace_uid: uuid::Uuid::new_v4().to_string(),
        origin: OpportunityOriginV1::Roadmap,
        kind: OpportunityKindV1::NewFeature,
        target: OpportunityTargetV1 {
            kind: OpportunityTargetKindV1::KooladeProduct,
            repository_uid: uuid::Uuid::new_v4().to_string(),
            canonical_remote: "https://github.com/zbarno/kool.ade".into(),
            source_sha: "a".repeat(40),
        },
        related_feature_uids: vec![],
        related_task_uids: vec![],
        discovered_at_ms: 10,
        observation_revision: 1,
        evidence_refs: vec![OpportunityEvidenceRefV1 {
            evidence_uid: uuid::Uuid::new_v4().to_string(),
            sha256: "b".repeat(64),
            confidentiality: EvidenceConfidentialityV1::Public,
        }],
        problem_or_opportunity: "Add a product improvement".into(),
        user_value: "Improve product workflows".into(),
        expected_outcome: "A reviewed feature is available".into(),
        possible_approaches: vec!["Use the existing lifecycle".into()],
        recommended_approach: "Use the existing lifecycle".into(),
        alternatives: vec!["Wait".into()],
        estimated_scope: OpportunityScopeV1::Small,
        impact: "Less manual effort".into(),
        cost_or_risk: "Ordinary implementation risk".into(),
        affected_systems: vec!["planning".into()],
        current_capability_state: "Not implemented".into(),
        generic_workaround: None,
        proposed_approach: "Create a reviewed feature goal".into(),
        dedup_fingerprint: "c".repeat(64),
        status: OpportunityStatusV1::Proposed,
        revision: 1,
    }
}

fn run(decision_uid: &str, proposal: &OpportunityV1) -> SelfImprovementRunV1 {
    let feature_uid = uuid::Uuid::new_v4().to_string();
    SelfImprovementRunV1 {
        schema_version: 1,
        run_uid: SelfImprovementStore::run_uid_for_decision(decision_uid).unwrap(),
        opportunity_uid: proposal.opportunity_uid.clone(),
        approved_decision_uid: decision_uid.into(),
        target_repository_uid: proposal.target.repository_uid.clone(),
        canonical_remote: proposal.target.canonical_remote.clone(),
        source_ref: None,
        source_sha: proposal.target.source_sha.clone(),
        destination_ref: None,
        feature_uid,
        task_uids: vec![],
        owner_task_uid: None,
        parent_task_progress: vec![],
        clone_uid: None,
        plan_approval_hash: None,
        independent_plan_review_uid: None,
        implementation_head_sha: None,
        implementation_subject_sha256: None,
        code_review_uids: vec![],
        runtime_qa_uids: vec![],
        spec_receipt_uid: None,
        pr_work_item_uid: None,
        pr_url: None,
        merged_sha: None,
        activated_version: None,
        phase: SelfImprovementPhaseV1::ApprovedToPlan,
        revision: 0,
    }
}

fn decision_for(proposal: &OpportunityV1) -> UiDecision {
    UiDecision {
        uid: uuid::Uuid::new_v4().to_string(),
        opportunity_uid: proposal.opportunity_uid.clone(),
        revision_hash: proposal.revision_hash().unwrap(),
        user_id: "local-controller".into(),
        decision: OpportunityChoice::Pursue,
        occurred_at: "2026-10-13T12:00:00Z".into(),
        snooze_until: None,
        explanation: Some("Synthetic UI action".into()),
    }
}

#[test]
fn pursued_product_decision_creates_one_durable_run_and_replay_is_idempotent() {
    let temp = TempDir::new();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.0.join("planning"),
        StoreMode::ManagedLocal,
    );
    let opportunities = OpportunityStore::new(&store);
    let proposal = opportunity();
    let saved = opportunities.save_advisory(proposal).unwrap();
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = saved
    else {
        panic!("first proposal should be created")
    };
    let decision = decision_for(&proposal);
    let decision_uid = decision.uid.clone();
    opportunities
        .record_user_decision(record_revision, &decision, "local-controller")
        .unwrap();
    let initial = run(&decision_uid, &proposal);
    let runs = SelfImprovementStore::new(&store);
    let created = runs
        .create_from_pursued(&opportunities, &decision_uid, initial.clone())
        .unwrap();
    assert_eq!(created.revision, 2);
    assert_eq!(created.phase, SelfImprovementPhaseV1::Planning);
    assert_eq!(runs.load(&created.run_uid).unwrap(), created);
    let (linked, _) = opportunities.load(&proposal.opportunity_uid).unwrap();
    assert_eq!(linked.status, OpportunityStatusV1::Planning);
    assert!(linked.related_feature_uids.contains(&created.feature_uid));
    assert_eq!(
        runs.create_from_pursued(&opportunities, &decision_uid, initial)
            .unwrap(),
        created
    );
    assert_eq!(runs.transitions(&created.run_uid).unwrap().len(), 2);
}

#[test]
fn durable_transitions_are_fenced_and_idempotent_after_replay() {
    let temp = TempDir::new();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.0.join("planning"),
        StoreMode::ManagedLocal,
    );
    let opportunities = OpportunityStore::new(&store);
    let proposal = opportunity();
    let saved = opportunities.save_advisory(proposal).unwrap();
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = saved
    else {
        panic!("first proposal should be created")
    };
    let decision = decision_for(&proposal);
    opportunities
        .record_user_decision(record_revision, &decision, "local-controller")
        .unwrap();
    let runs = SelfImprovementStore::new(&store);
    let created = runs
        .create_from_pursued(&opportunities, &decision.uid, run(&decision.uid, &proposal))
        .unwrap();
    let event_uid = uuid::Uuid::new_v4().to_string();
    let event = SelfImprovementEventV1::NeedsAttention;
    let updated = runs
        .apply_event(&created.run_uid, 2, &event_uid, event.clone())
        .unwrap();
    assert_eq!(updated.phase, SelfImprovementPhaseV1::NeedsAttention);
    assert_eq!(
        runs.apply_event(&created.run_uid, 2, &event_uid, event)
            .unwrap(),
        updated
    );
    assert!(
        runs.apply_event(
            &created.run_uid,
            2,
            &uuid::Uuid::new_v4().to_string(),
            SelfImprovementEventV1::NeedsAttention
        )
        .is_err()
    );
    assert_eq!(runs.transitions(&created.run_uid).unwrap().len(), 3);
}
