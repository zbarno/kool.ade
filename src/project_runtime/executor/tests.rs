use super::{
    artifact_capture_paths, join_workspace_path, next_timestamp, receipt_artifact_hashes,
    validate_manifest_candidate,
};
use crate::{
    execution::contract::WorkspaceRef,
    project_runtime::{
        AppSurface, AppTargetV1, ExecutionRecipeV1, NetworkRequirement,
        ProjectCapabilityManifestV1, RecipeSource, RuntimeOperation, TargetCapabilityState,
    },
};

fn approved_manifest() -> (ProjectCapabilityManifestV1, WorkspaceRef) {
    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let target_id = "web-app".to_owned();
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "web-build".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "node".into(),
        argv: vec!["build.js".into()],
        allowed_env_keys: vec![],
        allowed_output_paths: vec!["dist".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: vec![],
        cache_scope: None,
        prerequisite_tool_ids: vec!["node".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:approval-1".into()],
    };
    let target = AppTargetV1 {
        target_id,
        owning_task_id: task_id.clone(),
        repository_id: "repo-main".into(),
        app_surface: AppSurface::Web,
        project_relative_root: ".".into(),
        build_recipe_ref: Some("web-build".into()),
        test_recipe_refs: vec![],
        start_recipe_ref: None,
        readiness_probe_ref: None,
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: vec![],
        supported_driver_ids: vec![],
        port_needs: vec![],
        environment_policy_ref: "default".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
    let sha = "a".repeat(40);
    let manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: project_id.clone(),
        repository_id: "repo-main".into(),
        source_sha: sha.clone(),
        detected_languages: vec!["JavaScript".into()],
        frameworks: vec![],
        candidate_targets: vec![target],
        selected_target_id: None,
        target_capabilities: vec![TargetCapabilityState::ReadyButUntested],
        recipes: vec![recipe],
        readiness_probes: Default::default(),
        artifact_specs: Default::default(),
        confidence: 80,
        evidence_refs: vec!["package.json".into()],
        user_overrides: vec![],
        revision: 1,
    };
    let workspace = WorkspaceRef {
        project_id,
        task_id,
        source_sha: sha,
    };
    (manifest, workspace)
}

#[test]
fn target_working_directory_is_scoped_to_the_application_root() {
    assert_eq!(
        join_workspace_path("apps/web", "packages/client").unwrap(),
        "apps/web/packages/client"
    );
    assert_eq!(join_workspace_path(".", ".").unwrap(), ".");
}

#[test]
fn target_working_directory_rejects_absolute_and_parent_paths() {
    assert!(join_workspace_path("apps/web", "../../outside").is_err());
    assert!(join_workspace_path("/outside", ".").is_err());
}

#[test]
fn recipe_candidate_requires_exact_task_source_and_user_approval() {
    let (manifest, workspace) = approved_manifest();
    assert!(
        validate_manifest_candidate(&manifest, &workspace, "web-app", RuntimeOperation::Build)
            .is_ok()
    );

    let mut stale_workspace = workspace.clone();
    stale_workspace.source_sha = "b".repeat(40);
    assert!(
        validate_manifest_candidate(
            &manifest,
            &stale_workspace,
            "web-app",
            RuntimeOperation::Build
        )
        .unwrap_err()
        .to_string()
        .contains("pinned task workspace")
    );

    let mut another_task = workspace.clone();
    another_task.task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    assert!(
        validate_manifest_candidate(&manifest, &another_task, "web-app", RuntimeOperation::Build)
            .unwrap_err()
            .to_string()
            .contains("different task")
    );

    let mut unapproved = manifest;
    unapproved.recipes[0].evidence_refs.clear();
    assert!(
        validate_manifest_candidate(&unapproved, &workspace, "web-app", RuntimeOperation::Build)
            .unwrap_err()
            .to_string()
            .contains("explicit approval")
    );

    let mut forged_adapter = unapproved;
    forged_adapter.recipes[0].source = RecipeSource::TrustedAdapter;
    forged_adapter.recipes[0].evidence_refs = vec!["adapter:forged".into()];
    assert!(
        validate_manifest_candidate(
            &forged_adapter,
            &workspace,
            "web-app",
            RuntimeOperation::Build
        )
        .is_err()
    );
}

#[test]
fn receipt_timestamp_moves_past_a_same_millisecond_running_claim() {
    assert_eq!(next_timestamp(500, 500), 501);
}

#[test]
fn build_artifact_capture_uses_the_exact_target_spec_and_recipe_allowlist() {
    let (mut manifest, _) = approved_manifest();
    let target = &mut manifest.candidate_targets[0];
    target.artifact_spec_ref = Some("web-assets".into());
    manifest
        .artifact_specs
        .insert("web-assets".into(), vec!["dist/index.html".into()]);
    let target = manifest.candidate_targets[0].clone();
    let recipe = manifest.recipes[0].clone();

    assert_eq!(
        artifact_capture_paths(&manifest, &target, &recipe, RuntimeOperation::Build).unwrap(),
        ["dist/index.html"]
    );

    manifest
        .artifact_specs
        .insert("web-assets".into(), vec!["private/config.json".into()]);
    assert!(artifact_capture_paths(&manifest, &target, &recipe, RuntimeOperation::Build).is_err());
}

#[test]
fn monorepo_artifact_capture_stays_target_scoped_and_receipts_use_workspace_paths() {
    let (mut manifest, _) = approved_manifest();
    manifest.candidate_targets[0].project_relative_root = "apps/web".into();
    manifest.candidate_targets[0].artifact_spec_ref = Some("web-assets".into());
    manifest
        .artifact_specs
        .insert("web-assets".into(), vec!["dist/index.html".into()]);
    let target = manifest.candidate_targets[0].clone();
    let recipe = manifest.recipes[0].clone();
    let capture =
        artifact_capture_paths(&manifest, &target, &recipe, RuntimeOperation::Build).unwrap();

    assert_eq!(capture, ["dist/index.html"]);
    let receipt_hashes = receipt_artifact_hashes(
        &target.project_relative_root,
        &[crate::execution::contract::ArtifactManifestEntryV1 {
            relative_path: capture[0].clone(),
            size_bytes: 5,
            sha256: "a".repeat(64),
            media_type: "text/html".into(),
        }],
    )
    .unwrap();
    assert_eq!(receipt_hashes["apps/web/dist/index.html"], "a".repeat(64));

    manifest
        .artifact_specs
        .insert("web-assets".into(), vec!["private/config.json".into()]);
    assert!(artifact_capture_paths(&manifest, &target, &recipe, RuntimeOperation::Build).is_err());
}
