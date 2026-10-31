use super::super::*;
use crate::artifacts::planning_store::PlanningStore;
use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};

#[path = "custom_target/test_only.rs"]
mod test_only;

#[test]
fn custom_target_recipes_and_readiness_persist_as_untested_user_approved_state() {
    let root = std::env::temp_dir().join(format!("koolade-custom-target-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        &root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let runtime = ProjectRuntimeStore::new(&store);
    let workspace_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let _approval_cleanup =
        crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&workspace_id);
    let manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id,
        repository_id: "repo-custom".into(),
        source_sha: "a".repeat(40),
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
    let approval_proof = crate::ui::layout::RuntimeTargetApprovalProof::for_test();
    let receipt_id = approval_proof.receipt_uid();
    let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let approval_ref = format!("user-approval:{receipt_id}");
    let recipe = |recipe_id: &str, argv: Vec<&str>| ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "python".into(),
        argv: argv.into_iter().map(str::to_owned).collect(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: vec!["dist/index.html".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["python".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![
            approval_ref.clone(),
            format!("user-recipe-approval:{}", uuid::Uuid::new_v4().hyphenated()),
        ],
    };
    let target = AppTargetV1 {
        target_id: "user-target-unknown".into(),
        owning_task_id: task_uid.clone(),
        repository_id: "repo-custom".into(),
        app_surface: AppSurface::Web,
        project_relative_root: "apps/unknown".into(),
        build_recipe_ref: Some("custom-build".into()),
        test_recipe_refs: vec!["custom-test".into(), "custom-test-extra".into()],
        start_recipe_ref: Some("custom-start".into()),
        readiness_probe_ref: None,
        artifact_spec_ref: Some("custom-artifacts".into()),
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default-deny".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
    let readiness = Some(ReadinessProbeV1::HttpOnApprovedPort {
        port: 8080,
        path: "/health".into(),
        expected_status: 200,
    });
    let recipes = vec![
        recipe("custom-build", vec!["-m", "compileall", "."]),
        recipe("custom-test", vec!["-m", "unittest"]),
        recipe("custom-test-extra", vec!["-m", "doctest"]),
        recipe("custom-start", vec!["app.py"]),
    ];
    let receipt = GateReceipt {
        receipt_uid: receipt_id,
        subject_uid: target.owning_task_id.clone(),
        subject_sha256: ProjectRuntimeStore::user_target_proposal_sha256(
            &manifest.workspace_id,
            &manifest.repository_id,
            &manifest.source_sha,
            &target,
            &recipes,
            readiness.as_ref(),
            Some(&vec!["dist/index.html".into()]),
        )
        .unwrap(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1_800_000_000_000,
        evidence_refs: vec!["runtime-target:user-target-unknown".into()],
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
            readiness,
            vec!["dist/index.html".into()],
            receipt,
            approval_proof,
            1,
        )
        .unwrap();

    assert_eq!(saved.revision, 2);
    assert_eq!(saved.user_overrides.len(), 4);
    assert!(saved.is_active_target(&task_uid, "user-target-unknown"));
    let target = saved
        .candidate_targets
        .iter()
        .find(|target| target.target_id == "user-target-unknown")
        .unwrap();
    assert_eq!(
        target.capability_state,
        TargetCapabilityState::ReadyButUntested
    );
    assert_eq!(target.port_needs, [8080]);
    let probe_id = target.readiness_probe_ref.as_ref().unwrap();
    assert_eq!(
        saved.readiness_probes.get(probe_id),
        Some(&ReadinessProbeV1::HttpOnApprovedPort {
            port: 8080,
            path: "/health".into(),
            expected_status: 200,
        })
    );
    assert_eq!(runtime.load_manifest().unwrap(), Some(saved.clone()));
    runtime
        .verify_user_target_approval(&saved, target)
        .expect("execution boundary accepts the exact persisted approval");
    runtime
        .verify_execution_provenance(&saved, target, RuntimeOperation::Test)
        .expect("every referenced Test recipe has an exact local approval binding");
    let mut tampered = saved.clone();
    tampered.user_overrides[0].argv.push("--tampered".into());
    assert!(
        runtime
            .verify_user_target_approval(&tampered, target)
            .is_err()
    );
    let mut changed_artifact_allowlist = saved.clone();
    changed_artifact_allowlist
        .artifact_specs
        .get_mut("custom-artifacts")
        .unwrap()
        .push("dist/extra.html".into());
    assert!(
        runtime
            .verify_user_target_approval(&changed_artifact_allowlist, target)
            .is_err()
    );
    let mut wrong_task = target.clone();
    wrong_task.owning_task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    assert!(
        runtime
            .verify_user_target_approval(&saved, &wrong_task)
            .is_err()
    );
    let mut probe_bound_manifest = saved.clone();
    let start_target = probe_bound_manifest
        .candidate_targets
        .iter_mut()
        .find(|candidate| candidate.target_id == "user-target-unknown")
        .unwrap();
    let readiness_id = start_target.readiness_probe_ref.clone().unwrap();
    probe_bound_manifest.readiness_probes.insert(
        readiness_id,
        ReadinessProbeV1::CustomApprovedProbe {
            recipe_id: "approved-probe".into(),
        },
    );
    probe_bound_manifest
        .user_overrides
        .push(recipe("approved-probe", vec!["probe.py"]));
    let start_approval = probe_bound_manifest
        .user_overrides
        .iter_mut()
        .find(|candidate| candidate.recipe_id == "custom-start")
        .unwrap();
    start_approval
        .evidence_refs
        .retain(|reference| !reference.starts_with("user-recipe-approval:"));
    start_approval.evidence_refs.push(format!(
        "user-recipe-approval:{}",
        uuid::Uuid::new_v4().hyphenated()
    ));
    let start_target = probe_bound_manifest
        .candidate_targets
        .iter()
        .find(|candidate| candidate.target_id == "user-target-unknown")
        .unwrap();
    let start_recipe = probe_bound_manifest
        .user_overrides
        .iter()
        .find(|candidate| candidate.recipe_id == "custom-start")
        .unwrap();
    let start_binding = ProjectRuntimeStore::user_recipe_approval_binding(
        &probe_bound_manifest,
        start_target,
        RuntimeOperation::Start,
        start_recipe,
    )
    .unwrap();
    crate::persistence::runtime_target_approvals::save(&start_binding).unwrap();
    runtime
        .verify_user_recipe_approval(
            &probe_bound_manifest,
            start_target,
            RuntimeOperation::Start,
            start_recipe,
        )
        .expect("local Start approval binds the custom probe recipe contents");
    probe_bound_manifest
        .user_overrides
        .iter_mut()
        .find(|candidate| candidate.recipe_id == "approved-probe")
        .unwrap()
        .argv
        .push("--changed".into());
    let start_target = probe_bound_manifest
        .candidate_targets
        .iter()
        .find(|candidate| candidate.target_id == "user-target-unknown")
        .unwrap();
    let start_recipe = probe_bound_manifest
        .user_overrides
        .iter()
        .find(|candidate| candidate.recipe_id == "custom-start")
        .unwrap();
    assert!(
        runtime
            .verify_user_recipe_approval(
                &probe_bound_manifest,
                start_target,
                RuntimeOperation::Start,
                start_recipe,
            )
            .is_err()
    );
    let receipt_id = saved.user_overrides[0]
        .evidence_refs
        .iter()
        .find_map(|reference| reference.strip_prefix("user-approval:"))
        .unwrap();
    let binding = crate::persistence::runtime_target_approvals::Binding {
        workspace_id: saved.workspace_id.clone(),
        task_id: target.owning_task_id.clone(),
        repository_id: saved.repository_id.clone(),
        source_sha: saved.source_sha.clone(),
        target_id: target.target_id.clone(),
        proposal_sha256: ProjectRuntimeStore::user_target_proposal_sha256(
            &saved.workspace_id,
            &saved.repository_id,
            &saved.source_sha,
            target,
            &saved.user_overrides,
            saved.readiness_probes.get(probe_id),
            saved.artifact_specs.get("custom-artifacts"),
        )
        .unwrap(),
        receipt_id: receipt_id.to_owned(),
    };
    crate::persistence::runtime_target_approvals::revoke(&binding).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
