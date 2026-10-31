use super::*;
use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};

#[test]
// Production boundary: persist and verify a user-approved Test-only target
// through the file-backed runtime store. A validator alone cannot prove that
// approval hashes, manifest references, and local provenance allow no artifact
// spec. Uses temporary local state only; no Docker, SSH, network, or credentials.
fn test_only_custom_target_saves_without_build_artifacts() {
    let root = std::env::temp_dir().join(format!("koolade-test-only-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        &root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let runtime = ProjectRuntimeStore::new(&planning);
    let workspace_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let _approval_cleanup =
        crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&workspace_id);
    let manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id,
        repository_id: "repo-test-only".into(),
        source_sha: "b".repeat(40),
        detected_languages: Vec::new(),
        frameworks: Vec::new(),
        candidate_targets: Vec::new(),
        selected_target_id: None,
        target_capabilities: Vec::new(),
        recipes: Vec::new(),
        readiness_probes: Default::default(),
        artifact_specs: Default::default(),
        confidence: 0,
        evidence_refs: Vec::new(),
        user_overrides: Vec::new(),
        revision: 0,
    };
    runtime.save_manifest(&manifest, 0).unwrap();
    let proof = crate::ui::layout::RuntimeTargetApprovalProof::for_test();
    let receipt_id = proof.receipt_uid();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let target = AppTargetV1 {
        target_id: "user-target-test-only".into(),
        owning_task_id: task_id.clone(),
        repository_id: manifest.repository_id.clone(),
        app_surface: AppSurface::HeadlessService,
        project_relative_root: "services/api".into(),
        build_recipe_ref: None,
        test_recipe_refs: vec!["custom-test".into()],
        start_recipe_ref: None,
        readiness_probe_ref: None,
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default-deny".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "custom-test".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "python".into(),
        argv: vec!["-m".into(), "unittest".into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["python".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![
            format!("user-approval:{receipt_id}"),
            format!("user-recipe-approval:{}", uuid::Uuid::new_v4().hyphenated()),
        ],
    };
    let recipes = vec![recipe];
    let receipt = GateReceipt {
        receipt_uid: receipt_id,
        subject_uid: task_id,
        subject_sha256: ProjectRuntimeStore::user_target_proposal_sha256(
            &manifest.workspace_id,
            &manifest.repository_id,
            &manifest.source_sha,
            &target,
            &recipes,
            None,
            None,
        )
        .unwrap(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1_800_000_000_000,
        evidence_refs: vec![format!("runtime-target:{}", target.target_id)],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };

    let saved = runtime
        .add_user_target_with_readiness_and_save(
            target,
            recipes,
            None,
            Vec::new(),
            receipt,
            proof,
            1,
        )
        .unwrap();

    let saved_target = saved
        .candidate_targets
        .iter()
        .find(|candidate| candidate.target_id == "user-target-test-only")
        .unwrap();
    assert_eq!(saved_target.artifact_spec_ref, None);
    assert!(saved.artifact_specs.is_empty());
    runtime
        .verify_user_target_approval(&saved, saved_target)
        .expect("the Test-only approval covers the target without an artifact spec");
    runtime
        .verify_execution_provenance(&saved, saved_target, RuntimeOperation::Test)
        .expect("the Test recipe remains authorized without a Build artifact spec");
}
