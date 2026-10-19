use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};

use super::*;

pub(super) fn synchronize_app_run(
    app: &mut crate::app::root::KooladeApp,
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    runs: &crate::core::self_improvement::SelfImprovementStore<'_>,
    run_uid: &str,
) -> Result<(), String> {
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        return Err("fixture disconnected during self-improvement sync".into());
    };
    synchronize_run(project, lifecycle, runs, run_uid)
}

pub(super) fn assert_app_idle(app: &mut crate::app::root::KooladeApp) {
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    assert!(project.active_implementations.is_empty());
    assert!(project.active_turn.is_none());
}

pub(super) fn test_product_proposal(project: &mut crate::app::session::Project) -> String {
    use crate::core::opportunity::{
        EvidenceConfidentialityV1, OpportunityEvidenceRefV1, OpportunityKindV1,
        OpportunityOriginV1, OpportunitySaveResult, OpportunityScopeV1, OpportunityStatusV1,
        OpportunityStore, OpportunityTargetV1, OpportunityV1,
    };
    let planning = &project.state.planning_store;
    crate::artifacts::migration::bootstrap_product_with_store(
        &project.state.repo_root,
        planning,
        "Synthetic self-improvement sync fixture",
    )
    .unwrap();
    let workspace_uid = planning.project_id.hyphenated().to_string();
    let output = std::process::Command::new("git")
        .args([
            "-C",
            project.state.repo_root.to_str().unwrap(),
            "rev-parse",
            "HEAD",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let source_sha = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    let proposal = OpportunityV1 {
        schema_version: 1,
        opportunity_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        workspace_uid: workspace_uid.clone(),
        origin: OpportunityOriginV1::Roadmap,
        kind: OpportunityKindV1::Quality,
        target: OpportunityTargetV1 {
            kind: crate::core::opportunity::OpportunityTargetKindV1::KooladeProduct,
            repository_uid: crate::core::project_repos::stable_uid(
                &workspace_uid,
                "repository",
                "root",
            ),
            canonical_remote: "https://github.com/zbarno/kool.ade".into(),
            source_sha,
        },
        related_feature_uids: vec![],
        related_task_uids: vec![],
        discovered_at_ms: 1,
        observation_revision: 1,
        evidence_refs: vec![OpportunityEvidenceRefV1 {
            evidence_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
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
    let OpportunitySaveResult::Created { proposal, .. } = OpportunityStore::new(planning)
        .save_advisory(proposal)
        .unwrap()
    else {
        panic!("test opportunity should be created")
    };
    proposal.opportunity_uid
}

pub(super) fn user_approval(snapshot_uid: &str, hash: &str) -> GateReceipt {
    GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot_uid.into(),
        subject_sha256: hash.into(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture/user-confirmation".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    }
}

pub(super) fn independent_plan_review(
    feature_uid: &str,
    snapshot_uid: &str,
    hash: &str,
) -> GateReceipt {
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render};
    let manifest = render(&RoleInputV1 {
        role: PromptRole::PlanReviewer,
        workspace_uid: "fixture-workspace".into(),
        feature_uid: feature_uid.into(),
        task_uid: None,
        original_goal: "Persisted approval fixture".into(),
        approved_spec_ref: Some(snapshot_uid.into()),
        approved_spec_hash: Some(hash.into()),
        decision_refs: vec![],
        evidence_manifest: vec![],
        execution_policy_ref: "self-improvement-test/v1".into(),
        output_schema_version: PromptRole::PlanReviewer.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: "fixture-revision".into(),
            subject_hash: hash.into(),
            author_run_id: "00000000-0000-4000-8000-000000000006".into(),
            reviewer_run_id: "00000000-0000-4000-8000-000000000007".into(),
            author_session_id: Some("00000000-0000-4000-8000-000000000008".into()),
            reviewer_session_id: Some("00000000-0000-4000-8000-000000000009".into()),
            requirements_matrix: vec!["fixture requirement".into()],
            explicit_exclusions: vec![],
        },
    })
    .unwrap()
    .manifest;
    GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot_uid.into(),
        subject_sha256: hash.into(),
        actor: GateActor::IndependentReviewer,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture/independent-review".into()],
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some("00000000-0000-4000-8000-000000000007".into()),
        author_run_id: Some("00000000-0000-4000-8000-000000000006".into()),
        reviewer_session_id: Some("00000000-0000-4000-8000-000000000009".into()),
        author_session_id: Some("00000000-0000-4000-8000-000000000008".into()),
    }
}
