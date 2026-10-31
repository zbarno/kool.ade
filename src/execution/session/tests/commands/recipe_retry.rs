use super::super::*;
use super::interrupted_backend;
use std::sync::Arc;

#[test]
fn approved_recipe_retry_preserves_the_approval_and_artifact_request() {
    use crate::{
        artifacts::planning_store::PlanningStore,
        project_runtime::{
            AppSurface, AppTargetV1, ExecutionRecipeV1, NetworkRequirement,
            ProjectCapabilityManifestV1, ProjectRuntimeStore, RecipeSource, RuntimeOperation,
            TargetCapabilityState,
        },
    };

    let root = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    let workspace = WorkspaceRef {
        project_id: "00000000-0000-4000-8000-000000000001".into(),
        task_id: "00000000-0000-4000-8000-000000000002".into(),
        source_sha: "a".repeat(40),
    };
    let planning = PlanningStore::new(
        uuid::Uuid::parse_str(&workspace.project_id).unwrap(),
        root.path(),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let runtime = ProjectRuntimeStore::new(&planning);
    let approval_proof = crate::ui::layout::RuntimeTargetApprovalProof::for_test();
    let approval_ref = format!("user-approval:{}", approval_proof.receipt_uid());
    let approved_recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "web-build".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "npm".into(),
        argv: vec!["run".into(), "build".into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: vec!["dist".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: vec!["dist/index.html".into()],
        cache_scope: None,
        prerequisite_tool_ids: Vec::new(),
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![approval_ref.clone()],
    };
    runtime
        .save_manifest(
            &ProjectCapabilityManifestV1 {
                schema_version: 1,
                workspace_id: workspace.project_id.clone(),
                repository_id: "repo-main".into(),
                source_sha: workspace.source_sha.clone(),
                detected_languages: Vec::new(),
                frameworks: Vec::new(),
                candidate_targets: vec![AppTargetV1 {
                    target_id: "web".into(),
                    owning_task_id: workspace.task_id.clone(),
                    repository_id: "repo-main".into(),
                    app_surface: AppSurface::Web,
                    project_relative_root: "apps/web".into(),
                    build_recipe_ref: None,
                    test_recipe_refs: Vec::new(),
                    start_recipe_ref: None,
                    readiness_probe_ref: None,
                    artifact_spec_ref: None,
                    fixture_ref: None,
                    required_dependencies: Vec::new(),
                    supported_driver_ids: Vec::new(),
                    port_needs: Vec::new(),
                    environment_policy_ref: "env-policy".into(),
                    capability_state: TargetCapabilityState::ReadyButUntested,
                }],
                selected_target_id: Some("web".into()),
                target_capabilities: vec![TargetCapabilityState::ReadyButUntested],
                recipes: Vec::new(),
                readiness_probes: Default::default(),
                artifact_specs: Default::default(),
                confidence: 100,
                evidence_refs: Vec::new(),
                user_overrides: Vec::new(),
                revision: 0,
            },
            0,
        )
        .unwrap();
    let approved_manifest = runtime
        .approve_recipe_and_save(
            "web",
            RuntimeOperation::Build,
            approved_recipe.clone(),
            approval_proof,
            1,
        )
        .unwrap();
    let approved_recipe = approved_manifest
        .user_overrides
        .iter()
        .find(|recipe| recipe.recipe_id == "web-build")
        .unwrap()
        .clone();

    let interrupted = interrupted_backend();
    let replacement = backend();
    let original_session = ExecutionSession::new(
        interrupted.clone(),
        workspace.clone(),
        ExecutionTarget::local(),
        ExecutionModeV1::Verification,
        format!("sha256:{}", "b".repeat(64)),
        ResourceBudgetV1 {
            cpu_millis: 1000,
            memory_bytes: 1024 * 1024 * 1024,
            process_count: 32,
            temporary_disk_bytes: 1024 * 1024 * 1024,
            output_bytes: 1024 * 1024,
        },
    )
    .unwrap();
    original_session.set_telemetry_attribution("codex", Some("gpt-5-codex"));
    let replacement_session = Arc::new(
        ExecutionSession::new(
            replacement.clone(),
            workspace,
            ExecutionTarget::local(),
            ExecutionModeV1::Verification,
            format!("sha256:{}", "b".repeat(64)),
            ResourceBudgetV1 {
                cpu_millis: 1000,
                memory_bytes: 1024 * 1024 * 1024,
                process_count: 32,
                temporary_disk_bytes: 1024 * 1024 * 1024,
                output_bytes: 1024 * 1024,
            },
        )
        .unwrap(),
    );
    let next = replacement_session.clone();
    original_session
        .set_remote_retry_factory(Arc::new(move || Ok(next.clone())))
        .unwrap();
    let workspace_recipe = ExecutionRecipeV1 {
        working_directory_relative: "apps/web".into(),
        allowed_output_paths: vec!["apps/web/dist".into()],
        expected_artifacts: vec!["apps/web/dist/index.html".into()],
        ..approved_recipe
    };
    let artifacts = vec!["dist/index.html".to_owned()];

    let output = original_session
        .run_scoped_recipe_with_artifacts(
            &runtime,
            "web",
            &workspace_recipe,
            "apps/web",
            &artifacts,
        )
        .unwrap();

    assert_eq!(output.result.exit_code, Some(0));
    assert_eq!(
        replacement_session.telemetry_attribution_for_test(),
        Some(("codex".into(), Some("gpt-5-codex".into())))
    );
    assert!(!interrupted.workspace_result_applied.load(Ordering::Acquire));
    let request = replacement.request.lock().unwrap().clone().unwrap();
    assert_eq!(request.argv, ["npm", "run", "build"]);
    assert_eq!(request.working_directory_relative, ".");
    assert_eq!(request.policy.artifact_paths(), artifacts);
    assert_eq!(request.mode, ExecutionModeV1::Verification);
    assert!(request.policy.network_service_ports().is_empty());
    assert_eq!(
        request.policy.resource_limits(),
        &ResourceBudgetV1 {
            cpu_millis: 1000,
            memory_bytes: 1024 * 1024 * 1024,
            process_count: 32,
            temporary_disk_bytes: 1024 * 1024 * 1024,
            output_bytes: 1024 * 1024,
        }
    );
    let approved_command = request.policy.approved_recipe_command().unwrap();
    assert_eq!(approved_command.approval_ref, approval_ref);
    assert_eq!(approved_command.executable_id, "npm");
    assert_eq!(approved_command.argv, ["run", "build"]);
    assert!(approved_command.allowed_environment_names.is_empty());
    assert_eq!(
        request
            .policy
            .workspace_write_scope()
            .unwrap()
            .root_relative,
        "apps/web"
    );
}
