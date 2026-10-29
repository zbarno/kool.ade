use super::super::*;
use std::path::Path;

#[path = "discovery/unsupported.rs"]
mod unsupported;

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "koolade-runtime-discovery-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn context() -> DiscoveryContext {
        DiscoveryContext {
            workspace_id: "workspace-1".into(),
            repository_id: "repo-1".into(),
            owning_task_id: "task-1".into(),
            source_sha: "a".repeat(40),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn discovers_nested_polyglot_targets_from_metadata_without_running_scripts() {
    let fixture = Fixture::new();
    fixture.write(
        "apps/web/package.json",
        r#"{"scripts":{"build":"touch executed"},"dependencies":{"vite":"*"}}"#,
    );
    fixture.write(
        "services/api/pyproject.toml",
        "[project]\ndependencies = ['fastapi']\n",
    );
    fixture.write("tools/worker/go.mod", "module example.test/worker\n");

    let evidence = detect_candidate_targets(&fixture.0).unwrap();
    let surfaces = evidence
        .iter()
        .map(|item| (item.evidence_path.as_str(), item.candidate_surface))
        .collect::<Vec<_>>();
    assert!(surfaces.contains(&("apps/web/package.json", AppSurface::Web)));
    assert!(surfaces.contains(&("services/api/pyproject.toml", AppSurface::Web)));
    assert!(surfaces.contains(&("tools/worker/go.mod", AppSurface::HeadlessService)));
    assert!(!fixture.0.join("apps/web/executed").exists());
}

#[test]
fn classifies_linux_capable_desktop_targets_and_marks_windows_only_targets_unsupported() {
    let fixture = Fixture::new();
    fixture.write(
        "desktop/app.csproj",
        "<Project><PackageReference Include=\"Avalonia\" /></Project>",
    );
    fixture.write(
        "windows/app.csproj",
        "<Project><UseWPF>true</UseWPF></Project>",
    );
    fixture.write("flutter/pubspec.yaml", "name: sample\n");
    std::fs::create_dir_all(fixture.0.join("flutter/linux")).unwrap();

    let evidence = detect_candidate_targets(&fixture.0).unwrap();
    let by_path = evidence
        .iter()
        .map(|item| (item.evidence_path.as_str(), item.candidate_surface))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(by_path["desktop/app.csproj"], AppSurface::LinuxDesktop);
    assert_eq!(by_path["windows/app.csproj"], AppSurface::UnsupportedTarget);
    assert_eq!(by_path["flutter/pubspec.yaml"], AppSurface::LinuxDesktop);
}

#[test]
fn discovery_never_reads_outside_the_selected_root_through_symlinked_directories() {
    let fixture = Fixture::new();
    let external = fixture.0.with_extension("external");
    std::fs::create_dir_all(&external).unwrap();
    std::fs::write(external.join("Cargo.toml"), "[package]\nname='outside'\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&external, fixture.0.join("linked")).unwrap();
    assert!(
        detect_candidate_targets(Path::new(&fixture.0))
            .unwrap()
            .is_empty()
    );
    let _ = std::fs::remove_dir_all(external);
}

#[test]
fn manifest_discovery_is_stack_neutral_advisory_and_deterministic() {
    let fixture = Fixture::new();
    fixture.write("desktop/Cargo.toml", "[dependencies]\neframe = '0.1'\n");
    fixture.write("web/package.json", r#"{"dependencies":{"vite":"*"}}"#);
    fixture.write(
        "api/pyproject.toml",
        "[project]\ndependencies=['fastapi']\n",
    );
    fixture.write("service/go.mod", "module example.test/service\n");

    let first = discover_manifest(&fixture.0, &Fixture::context()).unwrap();
    let second = discover_manifest(&fixture.0, &Fixture::context()).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first.detected_languages,
        ["Go", "JavaScript", "Python", "Rust"]
    );
    assert_eq!(first.candidate_targets.len(), 4);
    assert!(first.candidate_targets.iter().all(|target| {
        target.build_recipe_ref.is_none()
            && target.test_recipe_refs.is_empty()
            && target.capability_state == TargetCapabilityState::ReadyButUntested
    }));
    assert!(
        first
            .candidate_targets
            .iter()
            .any(|target| target.app_surface == AppSurface::LinuxDesktop)
    );
    assert!(
        first
            .candidate_targets
            .iter()
            .any(|target| target.app_surface == AppSurface::Web)
    );
    assert!(
        first
            .candidate_targets
            .iter()
            .any(|target| target.app_surface == AppSurface::HeadlessService)
    );
}

#[test]
fn checked_in_runtime_matrix_fixtures_discover_deterministically_without_toolchains() {
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_qa");
    let cases = [
        ("rust_desktop", "Rust", AppSurface::LinuxDesktop),
        ("typescript_web", "JavaScript", AppSurface::Web),
        ("python_web", "Python", AppSurface::Web),
        ("go_service", "Go", AppSurface::HeadlessService),
    ];
    for (directory, language, surface) in cases {
        let root = fixture_root.join(directory);
        let first = discover_manifest(&root, &Fixture::context()).unwrap();
        let second = discover_manifest(&root, &Fixture::context()).unwrap();
        assert_eq!(first, second, "fixture {directory}");
        assert_eq!(first.detected_languages, [language], "fixture {directory}");
        assert_eq!(first.candidate_targets.len(), 1, "fixture {directory}");
        assert_eq!(first.candidate_targets[0].app_surface, surface);
        assert!(first.recipes.is_empty());
        assert!(first.user_overrides.is_empty());
    }

    let polyglot = discover_manifest(&fixture_root.join("polyglot"), &Fixture::context()).unwrap();
    assert_eq!(polyglot.candidate_targets.len(), 2);
    assert_eq!(polyglot.detected_languages, ["JavaScript", "Python"]);
    assert!(polyglot.candidate_targets.iter().any(|target| {
        target.project_relative_root == "apps/web" && target.app_surface == AppSurface::Web
    }));
    assert!(polyglot.candidate_targets.iter().any(|target| {
        target.project_relative_root == "services/api" && target.app_surface == AppSurface::Web
    }));
    assert!(
        polyglot
            .evidence_refs
            .contains(&"apps/web/src/index.html".to_owned())
    );
}

#[test]
fn explicit_recipe_override_authorizes_only_the_selected_target_and_operation() {
    let fixture = Fixture::new();
    fixture.write("web/package.json", r#"{"dependencies":{"vite":"*"}}"#);
    let mut manifest = discover_manifest(&fixture.0, &Fixture::context()).unwrap();
    let target_id = manifest.candidate_targets[0].target_id.clone();
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "approved-build".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "node".into(),
        argv: vec!["build.mjs".into()],
        allowed_env_keys: vec![],
        allowed_output_paths: vec!["dist".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: vec!["dist/index.html".into()],
        cache_scope: None,
        prerequisite_tool_ids: vec!["node".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:decision-1".into()],
    };
    apply_user_recipe_override(
        &mut manifest,
        &target_id,
        RuntimeOperation::Build,
        recipe.clone(),
    )
    .unwrap();
    assert_eq!(
        manifest.candidate_targets[0].build_recipe_ref.as_deref(),
        Some("approved-build")
    );
    assert_eq!(manifest.user_overrides, std::slice::from_ref(&recipe));
    assert!(manifest.validate().is_ok());

    let before = manifest.clone();
    let mut unauthorized = recipe;
    unauthorized.recipe_id = "metadata-build".into();
    unauthorized.source = RecipeSource::ValidatedProjectConfig;
    assert!(
        apply_user_recipe_override(
            &mut manifest,
            &target_id,
            RuntimeOperation::Build,
            unauthorized,
        )
        .is_err()
    );
    assert_eq!(manifest, before);
}

#[test]
fn unknown_stack_can_receive_an_explicit_web_target_without_changing_detection_code() {
    let fixture = Fixture::new();
    fixture.write("app/Makefile", "run: # inspected only\n");
    let mut manifest = discover_manifest(&fixture.0, &Fixture::context()).unwrap();
    let approval_ref = "user-approval:decision-7";
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "manual-build".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "project-builder".into(),
        argv: vec!["build".into(), "--offline".into()],
        allowed_env_keys: vec!["APP_MODE".into()],
        allowed_output_paths: vec!["dist".into()],
        timeout_seconds: 90,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["project-builder".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![approval_ref.into()],
    };
    let target = AppTargetV1 {
        target_id: "manual-web".into(),
        owning_task_id: "task-1".into(),
        repository_id: "repo-1".into(),
        app_surface: AppSurface::Web,
        project_relative_root: "app".into(),
        build_recipe_ref: Some("manual-build".into()),
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
    add_user_target_override(&mut manifest, target, vec![recipe], approval_ref).unwrap();
    manifest.validate().unwrap();
    assert!(
        manifest
            .candidate_targets
            .iter()
            .any(|target| target.target_id == "manual-web")
    );
    assert_eq!(manifest.detected_languages, ["Unknown"]);
    assert!(!fixture.0.join("app/dist").exists());
}
