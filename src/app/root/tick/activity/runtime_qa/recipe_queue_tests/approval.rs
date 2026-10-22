use super::*;

#[test]
fn queue_dispatch_requires_explicit_recipe_approval() {
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "web-build".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "npm".into(),
        argv: vec!["run".into(), "build".into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: vec!["./dist".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: vec!["./dist/index.html".into()],
        cache_scope: None,
        prerequisite_tool_ids: vec!["npm".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:reviewed-recipe".into()],
    };
    let target = AppTargetV1 {
        target_id: "web".into(),
        owning_task_id: "planning-item".into(),
        repository_id: "repo".into(),
        app_surface: AppSurface::Web,
        project_relative_root: "apps/web".into(),
        build_recipe_ref: Some("web-build".into()),
        test_recipe_refs: Vec::new(),
        start_recipe_ref: None,
        readiness_probe_ref: None,
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default".into(),
        capability_state: TargetCapabilityState::Ready,
    };
    let mut manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: "project".into(),
        repository_id: "repo".into(),
        source_sha: "a".repeat(40),
        detected_languages: vec!["JavaScript".into()],
        frameworks: Vec::new(),
        candidate_targets: vec![target.clone()],
        selected_target_id: None,
        target_capabilities: vec![TargetCapabilityState::Ready],
        recipes: vec![recipe.clone()],
        readiness_probes: Default::default(),
        artifact_specs: Default::default(),
        confidence: 80,
        evidence_refs: vec!["package.json".into()],
        user_overrides: Vec::new(),
        revision: 1,
    };
    let workspace = WorkspaceRef {
        project_id: "project".into(),
        task_id: "planning-item".into(),
        source_sha: "a".repeat(40),
    };
    let candidate = approved_candidate(
        &manifest,
        &target,
        std::path::Path::new("task-clone"),
        workspace.clone(),
        RuntimeOperation::Build,
        vec!["web-build"],
    )
    .unwrap()
    .unwrap();
    assert_eq!(candidate.required_tools, ["npm"]);
    assert_eq!(
        candidate.allowed_untracked_outputs,
        ["apps/web/dist", "apps/web/dist/index.html"]
    );

    let mut test_recipe = recipe.clone();
    test_recipe.recipe_id = "web-test".into();
    test_recipe.allowed_output_paths = vec!["test-results".into()];
    test_recipe.expected_artifacts = vec!["test-results/report.xml".into()];
    manifest.recipes.push(test_recipe);
    let mut later_test = recipe.clone();
    later_test.recipe_id = "web-test-later".into();
    later_test.allowed_output_paths = vec!["later-results".into()];
    later_test.expected_artifacts = vec!["later-results/report.xml".into()];
    manifest.recipes.push(later_test);
    let mut tested_target = target.clone();
    tested_target.test_recipe_refs = vec!["web-test".into(), "web-test-later".into()];
    let test_candidate = approved_candidate(
        &manifest,
        &tested_target,
        std::path::Path::new("task-clone"),
        workspace.clone(),
        RuntimeOperation::Test,
        vec!["web-test", "web-test-later"],
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        test_candidate.allowed_untracked_outputs,
        [
            "apps/web/dist",
            "apps/web/dist/index.html",
            "apps/web/test-results",
            "apps/web/test-results/report.xml"
        ]
    );

    for (executable, tool) in [("python", "python"), ("go", "go")] {
        manifest.recipes[0].executable_id = executable.into();
        manifest.recipes[0].prerequisite_tool_ids = vec![tool.into()];
        let selected = approved_candidate(
            &manifest,
            &target,
            std::path::Path::new("task-clone"),
            workspace.clone(),
            RuntimeOperation::Build,
            vec!["web-build"],
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.required_tools, [tool]);
    }

    manifest.recipes[0].executable_id = "custom-compiler".into();
    manifest.recipes[0].prerequisite_tool_ids = vec!["custom-compiler".into()];
    let custom = approved_candidate(
        &manifest,
        &target,
        std::path::Path::new("task-clone"),
        workspace.clone(),
        RuntimeOperation::Build,
        vec!["web-build"],
    )
    .unwrap()
    .unwrap();
    assert_eq!(custom.required_tools, ["custom-compiler"]);

    let lock = crate::execution::docker::images::ImageLock::bundled().unwrap();
    let profiles = [
        crate::execution::docker::images::RuntimeProfile::General,
        crate::execution::docker::images::RuntimeProfile::Rust,
        crate::execution::docker::images::RuntimeProfile::Node,
        crate::execution::docker::images::RuntimeProfile::Python,
        crate::execution::docker::images::RuntimeProfile::Go,
    ];
    let selected = lock
        .select_with_tools(&profiles, "amd64", &candidate.required_tools)
        .unwrap();
    assert_eq!(
        selected.profile,
        crate::execution::docker::images::RuntimeProfile::Node
    );
    assert!(
        lock.select_with_tools(&profiles, "amd64", &custom.required_tools)
            .is_err()
    );

    manifest.recipes[0].source = RecipeSource::ValidatedProjectConfig;
    manifest.recipes[0].evidence_refs.clear();
    assert!(
        approved_candidate(
            &manifest,
            &target,
            std::path::Path::new("task-clone"),
            workspace,
            RuntimeOperation::Build,
            vec!["web-build"]
        )
        .unwrap()
        .is_none()
    );

    manifest.recipes[0].source = RecipeSource::TrustedAdapter;
    let adapter = approved_candidate(
        &manifest,
        &target,
        std::path::Path::new("task-clone"),
        WorkspaceRef {
            project_id: "project".into(),
            task_id: "planning-item".into(),
            source_sha: "a".repeat(40),
        },
        RuntimeOperation::Build,
        vec!["web-build"],
    )
    .unwrap();
    assert!(
        adapter.is_some(),
        "verified adapters must reach executor provenance checks"
    );
}
