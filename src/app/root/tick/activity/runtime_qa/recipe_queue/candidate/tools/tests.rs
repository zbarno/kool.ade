use super::*;

#[test]
fn path_executable_is_not_resolved_as_an_image_tool_but_probe_tool_is() {
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "start-web".into(),
        source: crate::project_runtime::RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "target/release/server".into(),
        argv: Vec::new(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["python3".into()],
        network_requirement: crate::project_runtime::NetworkRequirement::ApprovedServicePorts,
        evidence_refs: vec!["user-approval:test".into()],
    };
    let target = AppTargetV1 {
        target_id: "web".into(),
        owning_task_id: "task".into(),
        repository_id: "repo".into(),
        app_surface: crate::project_runtime::AppSurface::Web,
        project_relative_root: ".".into(),
        build_recipe_ref: None,
        test_recipe_refs: Vec::new(),
        start_recipe_ref: Some(recipe.recipe_id.clone()),
        readiness_probe_ref: Some("ready".into()),
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: vec![8080],
        environment_policy_ref: "default-deny".into(),
        capability_state: crate::project_runtime::TargetCapabilityState::ReadyButUntested,
    };
    let manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: "workspace".into(),
        repository_id: "repo".into(),
        source_sha: "a".repeat(40),
        detected_languages: Vec::new(),
        frameworks: Vec::new(),
        candidate_targets: vec![target.clone()],
        selected_target_id: Some(target.target_id.clone()),
        target_capabilities: vec![target.capability_state.clone()],
        recipes: vec![recipe.clone()],
        readiness_probes: [(
            "ready".into(),
            ReadinessProbeV1::HttpOnApprovedPort {
                port: 8080,
                path: "/health".into(),
                expected_status: 200,
            },
        )]
        .into_iter()
        .collect(),
        artifact_specs: Default::default(),
        confidence: 100,
        evidence_refs: Vec::new(),
        user_overrides: Vec::new(),
        revision: 0,
    };

    let tools = required_tools(
        &manifest,
        &target,
        crate::project_runtime::RuntimeOperation::Start,
        &[&recipe],
    )
    .unwrap()
    .unwrap();
    assert_eq!(tools, ["python3"]);
}
