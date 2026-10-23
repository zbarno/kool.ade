use super::*;

fn add_product_proposal(project: &mut crate::app::session::Project) {
    use crate::core::opportunity::{
        EvidenceConfidentialityV1, OpportunityEvidenceRefV1, OpportunityKindV1,
        OpportunityOriginV1, OpportunitySaveResult, OpportunityScopeV1, OpportunityStatusV1,
        OpportunityStore, OpportunityTargetKindV1, OpportunityTargetV1, OpportunityV1,
    };
    let store = &project.state.planning_store;
    crate::artifacts::migration::bootstrap_product_with_store(
        &project.state.repo_root,
        store,
        "Synthetic plan review fixture",
    )
    .unwrap();
    let mut manifest = crate::core::project_repos::ProjectManifest::load(store).unwrap();
    manifest
        .repositories
        .iter_mut()
        .find(|repository| repository.id == "root")
        .unwrap()
        .remote = "https://github.com/zbarno/kool.ade".into();
    store
        .transaction(
            &[(
                crate::artifacts::planning_store::paths::PROJECT_MANIFEST.into(),
                serde_json::to_vec(&manifest).unwrap(),
            )],
            None,
        )
        .unwrap();
    project.state.repositories = manifest;
    let workspace_uid = store.project_id.hyphenated().to_string();
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
            kind: OpportunityTargetKindV1::KooladeProduct,
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
    assert!(matches!(
        OpportunityStore::new(store)
            .save_advisory(proposal)
            .unwrap(),
        OpportunitySaveResult::Created { .. }
    ));
}

/// Production boundary: the actual plan-review preparation path must refuse a persisted snapshot
/// until its matching user confirmation exists. A receipt-level unit test cannot prove that the
/// app's review dispatcher enforces this ordering. Uses a temporary app fixture and local stores;
/// no harness, network, or Docker. Setup cost is one headless board fixture.
#[test]
fn prepare_rejects_persisted_plan_without_current_user_confirmation() {
    let mut app = crate::app::root::board_tests::fixture();
    if let crate::app::root::Screen::Connected(project) = &mut app.screen {
        add_product_proposal(project);
    }
    let context = egui::Context::default();
    crate::app::root::board_tests::click_text(&mut app, &context, "Improvement proposals (1)");
    crate::app::root::board_tests::click_text(&mut app, &context, "Improve planning reliability");
    crate::app::root::board_tests::click_text(&mut app, &context, "Pursue");

    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    let (feature_id, markdown) = project
        .state
        .active_feature
        .as_ref()
        .expect("Pursue should create an active feature");
    let feature_id = feature_id.clone();
    let markdown = markdown.clone();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .expect("created feature should have an identity");
    let contract = crate::core::workflow::feature_contract(&markdown);
    let store = &project.state.planning_store;
    let initial = crate::core::lifecycle::PlanSnapshotV1::from_feature(
        &feature_id,
        &identity,
        &markdown,
        &contract,
    )
    .unwrap();
    crate::core::lifecycle::plan_snapshot::record_author(
        store,
        &crate::core::lifecycle::plan_snapshot::PlanAuthorProvenanceV1 {
            feature_uid: identity.uid.clone(),
            approval_contract_sha256: initial.approval_contract_sha256,
            author_run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            author_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        },
    )
    .unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        store,
        &feature_id,
        &identity,
        &markdown,
        &contract,
    )
    .unwrap();
    let (path, bytes, check) =
        crate::core::lifecycle::plan_snapshot::record_change(store, &snapshot)
            .unwrap()
            .expect("snapshot should be new");
    store
        .transaction_with_record_revisions(&[(path, bytes)], &[check])
        .unwrap();

    let error = PlanReview::prepare_for_test(project, feature_id).unwrap_err();
    assert_eq!(
        error,
        "independent plan review cannot start before current user confirmation"
    );
}
