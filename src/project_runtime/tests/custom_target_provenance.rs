use super::*;

#[test]
fn trusted_adapter_start_approval_binds_custom_probe_content() {
    let workspace_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let _approval_cleanup =
        crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&workspace_id);
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let recipe = |id: &str, argv: Vec<&str>| ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: id.into(),
        source: RecipeSource::TrustedAdapter,
        working_directory_relative: ".".into(),
        executable_id: "python".into(),
        argv: argv.into_iter().map(str::to_owned).collect(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["python".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: Vec::new(),
    };
    let target = AppTargetV1 {
        target_id: "discovered-target".into(),
        owning_task_id: task_id.clone(),
        repository_id: "repo-runtime".into(),
        app_surface: AppSurface::Web,
        project_relative_root: ".".into(),
        build_recipe_ref: None,
        test_recipe_refs: Vec::new(),
        start_recipe_ref: Some("start".into()),
        readiness_probe_ref: Some("probe-ref".into()),
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default-deny".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
    let mut manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: workspace_id.clone(),
        repository_id: "repo-runtime".into(),
        source_sha: "a".repeat(40),
        detected_languages: Vec::new(),
        frameworks: Vec::new(),
        candidate_targets: vec![target.clone()],
        selected_target_id: None,
        target_capabilities: vec![TargetCapabilityState::ReadyButUntested],
        recipes: vec![
            recipe("start", vec!["serve.py"]),
            recipe("probe", vec!["check.py"]),
        ],
        readiness_probes: [(
            "probe-ref".into(),
            ReadinessProbeV1::CustomApprovedProbe {
                recipe_id: "probe".into(),
            },
        )]
        .into(),
        artifact_specs: Default::default(),
        confidence: 0,
        evidence_refs: Vec::new(),
        user_overrides: Vec::new(),
        revision: 1,
    };
    let origin = ProjectRuntimeStore::trusted_adapter_origin_sha256(&manifest, &target).unwrap();
    crate::persistence::runtime_target_approvals::save_trusted_adapter_recipe_origin(
        &workspace_id,
        &task_id,
        &origin,
    )
    .unwrap();
    assert!(
        crate::persistence::runtime_target_approvals::has_trusted_adapter_recipe_origin(
            &workspace_id,
            &task_id,
            &origin,
        )
        .unwrap()
    );

    manifest.recipes[1].argv.push("--changed".into());
    let changed = ProjectRuntimeStore::trusted_adapter_origin_sha256(&manifest, &target).unwrap();
    assert_ne!(origin, changed);
    assert!(
        !crate::persistence::runtime_target_approvals::has_trusted_adapter_recipe_origin(
            &workspace_id,
            &task_id,
            &changed,
        )
        .unwrap()
    );

    manifest.recipes[1].source = RecipeSource::UserApproved;
    assert!(ProjectRuntimeStore::trusted_adapter_origin_sha256(&manifest, &target).is_err());
}

#[test]
fn discovered_target_origin_is_bound_to_workspace_and_source_revision() {
    let target = AppTargetV1 {
        target_id: "discovered-target".into(),
        owning_task_id: "task-runtime".into(),
        repository_id: "repo-runtime".into(),
        app_surface: AppSurface::Web,
        project_relative_root: "apps/site".into(),
        build_recipe_ref: None,
        test_recipe_refs: Vec::new(),
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
    let origin = ProjectRuntimeStore::discovered_target_origin_sha256(
        "workspace-a",
        &"a".repeat(40),
        &target,
    )
    .unwrap();
    assert_ne!(
        origin,
        ProjectRuntimeStore::discovered_target_origin_sha256(
            "workspace-b",
            &"a".repeat(40),
            &target,
        )
        .unwrap()
    );
    assert_ne!(
        origin,
        ProjectRuntimeStore::discovered_target_origin_sha256(
            "workspace-a",
            &"b".repeat(40),
            &target,
        )
        .unwrap()
    );
}
