use super::super::*;
use super::proposal;
use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    harness::prompt_catalog::{
        OpportunityChoice, OpportunityConfidence, OpportunityDecisionV1 as UiDecision,
        OpportunityKind, OpportunityOrigin, OpportunityProposalV1, OpportunityScope,
        OpportunityTarget,
    },
};

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("opportunity-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn prompt_decision(
    proposal: &OpportunityV1,
    user_id: &str,
    decision: OpportunityChoice,
) -> UiDecision {
    let snooze_until =
        (decision == OpportunityChoice::NotNow).then(|| "2026-10-20T12:00:00Z".to_owned());
    UiDecision {
        uid: uuid::Uuid::new_v4().to_string(),
        opportunity_uid: proposal.opportunity_uid.clone(),
        revision_hash: proposal.revision_hash().unwrap(),
        user_id: user_id.into(),
        decision,
        occurred_at: "2026-10-13T12:00:00Z".into(),
        snooze_until,
        explanation: Some("Synthetic user chose to pursue".into()),
    }
}

fn setup() -> (TempDir, PlanningStore) {
    let temp = TempDir::new();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.0.join("planning"),
        StoreMode::ManagedLocal,
    );
    (temp, store)
}

#[test]
fn exploration_settings_are_opt_in_durable_bounded_and_revision_checked() {
    let (_temp, planning) = setup();
    let opportunities = OpportunityStore::new(&planning);
    let defaults = opportunities.discovery_settings().unwrap();
    assert_eq!(defaults.revision, 0);
    assert_eq!(
        defaults.exploration,
        crate::core::opportunity::OpportunityExploreModeV1::Disabled
    );

    let opted_in = crate::core::opportunity::OpportunityDiscoverySettingsV1 {
        exploration: crate::core::opportunity::OpportunityExploreModeV1::OptedIn,
        ..defaults.clone()
    };
    let saved = opportunities
        .save_discovery_settings(&opted_in, defaults.revision)
        .unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(opportunities.discovery_settings().unwrap(), saved);
    assert!(matches!(
        opportunities.save_discovery_settings(&opted_in, defaults.revision),
        Err(crate::artifacts::planning_store::StoreError::StaleRecordRevision { .. })
    ));

    let invalid = crate::core::opportunity::OpportunityDiscoverySettingsV1 {
        daily_proposal_limit: 11,
        ..saved
    };
    assert!(opportunities.save_discovery_settings(&invalid, 1).is_err());

    let invalid = crate::core::opportunity::OpportunityDiscoverySettingsV1 {
        active_self_improvement_limit: 11,
        ..opportunities.discovery_settings().unwrap()
    };
    assert!(opportunities.save_discovery_settings(&invalid, 2).is_err());
}

#[test]
fn advisory_quota_is_atomic_and_scheduled_exploration_requires_opt_in() {
    let (_temp, planning) = setup();
    let opportunities = OpportunityStore::new(&planning);
    for index in 0..3 {
        let mut candidate = proposal();
        candidate.problem_or_opportunity = format!("Independent verified gap {index}");
        assert!(matches!(
            opportunities.save_advisory(candidate).unwrap(),
            OpportunitySaveResult::Created { .. }
        ));
    }
    let mut over_quota = proposal();
    over_quota.problem_or_opportunity = "A fourth verified gap".into();
    assert!(opportunities.save_advisory(over_quota).is_err());

    let mut scheduled = proposal();
    scheduled.origin = crate::core::opportunity::OpportunityOriginV1::ScheduledExploration;
    scheduled.problem_or_opportunity = "Scheduled roadmap exploration".into();
    assert!(opportunities.save_advisory(scheduled.clone()).is_err());

    let default_settings = opportunities.discovery_settings().unwrap();
    let opted_in = crate::core::opportunity::OpportunityDiscoverySettingsV1 {
        exploration: crate::core::opportunity::OpportunityExploreModeV1::OptedIn,
        daily_proposal_limit: 10,
        ..default_settings.clone()
    };
    opportunities
        .save_discovery_settings(&opted_in, default_settings.revision)
        .unwrap();
    assert!(matches!(
        opportunities.save_advisory(scheduled).unwrap(),
        OpportunitySaveResult::Created { .. }
    ));
}

fn prompt_proposal(value: &OpportunityV1) -> OpportunityProposalV1 {
    OpportunityProposalV1 {
        origin: OpportunityOrigin::RunFailure,
        kind: OpportunityKind::BugFix,
        target: OpportunityTarget::ActiveUserProject,
        target_repository: value.target.canonical_remote.clone(),
        target_repository_ref: value.target.repository_uid.clone(),
        related_feature_uids: value.related_feature_uids.clone(),
        related_task_uids: value.related_task_uids.clone(),
        observation_revision: value.observation_revision.to_string(),
        evidence_refs: value
            .evidence_refs
            .iter()
            .map(|evidence| evidence.evidence_uid.clone())
            .collect(),
        problem_or_opportunity: value.problem_or_opportunity.clone(),
        user_value: value.user_value.clone(),
        expected_outcome: value.expected_outcome.clone(),
        possible_approaches: value.possible_approaches.clone(),
        recommended_approach: value.recommended_approach.clone(),
        alternatives: value.alternatives.join("\n"),
        estimated_scope: OpportunityScope::Small,
        impact: value.impact.clone(),
        cost_or_risk: value.cost_or_risk.clone(),
        affected_systems: value.affected_systems.clone(),
        current_capability_state: value.current_capability_state.clone(),
        generic_workaround: value.generic_workaround.clone(),
        proposed_approach: value.proposed_approach.clone(),
        dedup_fingerprint: value.dedup_fingerprint.clone(),
        inference: "Synthetic advisory output".into(),
        confidence: OpportunityConfidence::Low,
        speculative: false,
        generic_capability_check: "Checked current workspace capability".into(),
        deduplication_basis: "Same workspace, target, and normalized problem".into(),
    }
}

#[test]
fn advisory_proposals_are_deduplicated_across_store_reopen() {
    let (_temp, store) = setup();
    let opportunities = OpportunityStore::new(&store);
    let first = proposal();
    let created = opportunities.save_advisory(first.clone()).unwrap();
    let OpportunitySaveResult::Created {
        proposal: saved,
        record_revision,
    } = created
    else {
        panic!("first save should create a proposal")
    };
    let decision = prompt_decision(&saved, "synthetic-user", OpportunityChoice::Decline);
    opportunities
        .record_user_decision(record_revision, &decision, "synthetic-user")
        .unwrap();
    let (declined, _) = opportunities.load(&saved.opportunity_uid).unwrap();
    let reopened = OpportunityStore::new(&store);
    let mut duplicate = first;
    duplicate.opportunity_uid = uuid::Uuid::new_v4().to_string();
    duplicate.problem_or_opportunity = " repeated provider discovery failures ".into();
    let result = reopened.save_advisory(duplicate).unwrap();
    assert!(matches!(result, OpportunitySaveResult::Duplicate { .. }));
    let proposals = reopened.list().unwrap();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0], declined);
    assert_eq!(proposals[0].status, OpportunityStatusV1::Declined);
    assert!(reopened.approved_to_plan().unwrap().is_empty());
}

#[test]
fn typed_prompt_proposal_is_persisted_as_advisory_source() {
    let (_temp, store) = setup();
    let opportunities = OpportunityStore::new(&store);
    let value = proposal();
    let prompt = prompt_proposal(&value);
    let OpportunitySaveResult::Created { proposal, .. } =
        opportunities.save_prompt_advisory(value, prompt).unwrap()
    else {
        panic!("prompt proposal should be created")
    };
    let bytes = store
        .read(format!(
            "state/opportunities/{}.json",
            proposal.opportunity_uid
        ))
        .unwrap();
    let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        record
            .get("promptProposal")
            .is_some_and(|value| !value.is_null())
    );
    assert_eq!(proposal.status, OpportunityStatusV1::AwaitingDecision);
    assert!(opportunities.approved_to_plan().unwrap().is_empty());
}

#[path = "store/decisions.rs"]
mod decisions;
