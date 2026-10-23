use super::*;

pub(super) fn app_proposal(
    project: &mut crate::app::session::Project,
    target_kind: OpportunityTargetKindV1,
) -> String {
    let planning = &project.state.planning_store;
    crate::artifacts::migration::bootstrap_product_with_store(
        &project.state.repo_root,
        planning,
        "Synthetic Kool.ad/e fixture",
    )
    .unwrap();
    let workspace_uid = planning.project_id.hyphenated().to_string();
    let mut manifest = crate::core::project_repos::ProjectManifest::load(planning).unwrap();
    let repository = manifest
        .repositories
        .iter_mut()
        .find(|repository| repository.id == "root")
        .unwrap();
    repository.remote = "https://github.com/zbarno/kool.ade".into();
    project.state.repositories = manifest.clone();
    planning
        .transaction(
            &[(
                crate::artifacts::planning_store::paths::PROJECT_MANIFEST.to_owned(),
                serde_json::to_vec(&manifest).unwrap(),
            )],
            None,
        )
        .unwrap();
    project.state.identity = crate::domain::user::ResolvedIdentity {
        user: crate::domain::user::CurrentUser::new("Koolade Acceptance", Vec::new()),
        source: crate::domain::user::IdentitySource::ConfigBlock,
    };
    project.state.baseline_planning_revision = planning.revision().unwrap();

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
            kind: target_kind,
            repository_uid: crate::core::project_repos::stable_uid(
                &workspace_uid,
                "repository",
                "root",
            ),
            canonical_remote: "https://github.com/zbarno/kool.ade".into(),
            source_sha,
        },
        related_feature_uids: Vec::new(),
        related_task_uids: Vec::new(),
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
    let store = OpportunityStore::new(planning);
    let OpportunitySaveResult::Created { proposal, .. } = store.save_advisory(proposal).unwrap()
    else {
        panic!("fixture opportunity should be created")
    };
    proposal.opportunity_uid
}

pub(super) fn app_product_proposal(project: &mut crate::app::session::Project) -> String {
    app_proposal(project, OpportunityTargetKindV1::KooladeProduct)
}
