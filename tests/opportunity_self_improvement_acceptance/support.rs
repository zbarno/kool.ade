use std::{path::Path, process::Command};

use koolade::{
    core::{
        opportunity::{
            EvidenceConfidentialityV1, OpportunityEvidenceRefV1, OpportunityKindV1,
            OpportunityOriginV1, OpportunityScopeV1, OpportunityStatusV1, OpportunityTargetKindV1,
            OpportunityTargetV1, OpportunityV1,
        },
        self_improvement::{
            SelfImprovementEventV1 as Event, SelfImprovementPhaseV1 as Phase, SelfImprovementRunV1,
            SelfImprovementStore,
        },
    },
    harness::prompt_catalog::{OpportunityChoice, OpportunityDecisionV1 as UiDecision},
};

pub(super) fn uid() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

pub(super) fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Koolade Acceptance",
            "-c",
            "user.email=acceptance@example.invalid",
            "-C",
        ])
        .arg(repo)
        .args(args)
        .output()
        .expect("git is available for the isolated clone fixture");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub(super) fn proposal(source_sha: &str) -> OpportunityV1 {
    OpportunityV1 {
        schema_version: 1,
        opportunity_uid: uid(),
        workspace_uid: uid(),
        origin: OpportunityOriginV1::ScheduledExploration,
        kind: OpportunityKindV1::NewFeature,
        target: OpportunityTargetV1 {
            kind: OpportunityTargetKindV1::KooladeProduct,
            repository_uid: uid(),
            canonical_remote: "https://github.com/zbarno/kool.ade".into(),
            source_sha: source_sha.into(),
        },
        related_feature_uids: vec![],
        related_task_uids: vec![],
        discovered_at_ms: 1,
        observation_revision: 1,
        evidence_refs: vec![OpportunityEvidenceRefV1 {
            evidence_uid: uid(),
            sha256: "b".repeat(64),
            confidentiality: EvidenceConfidentialityV1::Public,
        }],
        problem_or_opportunity: "Add a useful product improvement".into(),
        user_value: "Reduce repeated manual work".into(),
        expected_outcome: "A reviewed capability is available".into(),
        possible_approaches: vec!["Use the normal feature lifecycle".into()],
        recommended_approach: "Plan and review the change normally".into(),
        alternatives: vec!["Defer the idea".into()],
        estimated_scope: OpportunityScopeV1::Small,
        impact: "Improves the user workflow".into(),
        cost_or_risk: "Ordinary product change risk".into(),
        affected_systems: vec!["planning".into()],
        current_capability_state: "Not available".into(),
        generic_workaround: None,
        proposed_approach: "Create an ordinary approved feature plan".into(),
        dedup_fingerprint: "c".repeat(64),
        status: OpportunityStatusV1::Proposed,
        revision: 1,
    }
}

pub(super) fn pursue_decision(proposal: &OpportunityV1) -> UiDecision {
    UiDecision {
        uid: uid(),
        opportunity_uid: proposal.opportunity_uid.clone(),
        revision_hash: proposal.revision_hash().unwrap(),
        user_id: "local-controller".into(),
        decision: OpportunityChoice::Pursue,
        occurred_at: "2026-10-13T12:00:00Z".into(),
        snooze_until: None,
        explanation: Some("Synthetic explicit Pursue action".into()),
    }
}

pub(super) fn self_improvement_run(
    decision_uid: &str,
    proposal: &OpportunityV1,
    source_sha: &str,
) -> SelfImprovementRunV1 {
    SelfImprovementRunV1 {
        schema_version: 1,
        run_uid: SelfImprovementStore::run_uid_for_decision(decision_uid).unwrap(),
        opportunity_uid: proposal.opportunity_uid.clone(),
        approved_decision_uid: decision_uid.into(),
        target_repository_uid: proposal.target.repository_uid.clone(),
        canonical_remote: proposal.target.canonical_remote.clone(),
        source_ref: None,
        source_sha: source_sha.into(),
        destination_ref: None,
        feature_uid: uid(),
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
        phase: Phase::ApprovedToPlan,
        revision: 0,
    }
}

pub(super) fn apply(
    store: &SelfImprovementStore<'_>,
    run: SelfImprovementRunV1,
    event: Event,
) -> SelfImprovementRunV1 {
    store
        .apply_event(&run.run_uid, run.revision, &uid(), event)
        .unwrap()
}
