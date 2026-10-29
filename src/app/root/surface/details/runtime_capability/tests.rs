use std::collections::BTreeSet;

use crate::{
    project_runtime::{
        AppSurface, AppTargetV1, ExecutionRecipeV1, NetworkRequirement,
        ProjectCapabilityManifestV1, ReadinessProbeV1, RecipeSource, TargetCapabilityState,
    },
    project_runtime_capability::QaApplicability,
};

use super::*;

#[test]
fn target_language_uses_only_manifest_evidence_in_the_target_root() {
    let evidence = vec![
        "apps/web/package.json".to_owned(),
        "services/api/go.mod".to_owned(),
        "Cargo.toml".to_owned(),
        "apps/unknown/project.yaml".to_owned(),
    ];

    assert_eq!(
        target_language(
            &crate::project_runtime::target_id_for_evidence_path("apps/web/package.json"),
            &evidence,
        )
        .as_deref(),
        Some("JavaScript")
    );
    assert_eq!(
        target_language(
            &crate::project_runtime::target_id_for_evidence_path("services/api/go.mod"),
            &evidence,
        )
        .as_deref(),
        Some("Go")
    );
    assert_eq!(
        target_language(
            &crate::project_runtime::target_id_for_evidence_path("Cargo.toml"),
            &evidence,
        )
        .as_deref(),
        Some("Rust")
    );
    assert_eq!(target_language("unknown-target", &evidence), None);
}

#[test]
fn target_language_does_not_confuse_same_root_polyglot_candidates() {
    let evidence = vec!["Cargo.toml".to_owned(), "package.json".to_owned()];
    let rust_target = crate::project_runtime::target_id_for_evidence_path("Cargo.toml");
    let node_target = crate::project_runtime::target_id_for_evidence_path("package.json");

    assert_ne!(rust_target, node_target);
    assert_eq!(
        target_language(&rust_target, &evidence).as_deref(),
        Some("Rust")
    );
    assert_eq!(
        target_language(&node_target, &evidence).as_deref(),
        Some("JavaScript")
    );
}

#[test]
fn surface_applicability_keeps_service_and_unsupported_targets_out_of_visual_qa() {
    assert_eq!(
        surface_applicability(AppSurface::Web),
        QaApplicability::Visual
    );
    assert_eq!(
        surface_applicability(AppSurface::LinuxDesktop),
        QaApplicability::Visual
    );
    assert_eq!(
        surface_applicability(AppSurface::HeadlessService),
        QaApplicability::ServiceEvidence
    );
    assert_eq!(
        surface_applicability(AppSurface::UnsupportedTarget),
        QaApplicability::Unsupported
    );
}

#[test]
fn projection_diagnostics_cover_missing_capabilities_without_granting_readiness() {
    let manifest = manifest();
    let task_id = "task-1";
    let capabilities = LockedCapabilities {
        tool_ids: BTreeSet::new(),
        profile_tool_ids: Vec::new(),
        driver_ids: BTreeSet::new(),
    };
    let dependencies = BTreeSet::new();

    let web_diagnostics =
        capability_diagnostics(&manifest, "web", task_id, &capabilities, &dependencies).unwrap();
    assert!(
        web_diagnostics
            .iter()
            .any(|message| message.contains("node"))
    );
    assert!(
        web_diagnostics
            .iter()
            .any(|message| message.contains("npm:fixture"))
    );
    assert!(
        web_diagnostics
            .iter()
            .any(|message| message.contains("web-driver"))
    );

    let unsupported_diagnostics = capability_diagnostics(
        &manifest,
        "unsupported",
        task_id,
        &capabilities,
        &dependencies,
    )
    .unwrap();
    assert!(
        unsupported_diagnostics
            .iter()
            .any(|message| message.contains("no supported runtime driver"))
    );
    assert_eq!(
        manifest.candidate_targets[0].capability_state,
        TargetCapabilityState::ReadyButUntested
    );
}

#[test]
fn recipe_tool_diagnostics_require_one_locked_profile_for_the_full_tool_set() {
    let mut manifest = manifest();
    manifest.recipes[0].prerequisite_tool_ids = vec!["node".into(), "python".into()];
    let capabilities = LockedCapabilities {
        tool_ids: ["node".into(), "python".into()].into(),
        profile_tool_ids: vec![["node".into()].into(), ["python".into()].into()],
        driver_ids: ["web-driver".into()].into(),
    };
    let dependencies = ["npm:fixture".into()].into();

    let diagnostics =
        capability_diagnostics(&manifest, "web", "task-1", &capabilities, &dependencies).unwrap();

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].contains("no single locked executor image"));
    assert!(diagnostics[0].contains("node, python"));

    let compatible_capabilities = LockedCapabilities {
        profile_tool_ids: vec![["node".into(), "python".into()].into()],
        ..capabilities
    };
    let compatible_diagnostics = capability_diagnostics(
        &manifest,
        "web",
        "task-1",
        &compatible_capabilities,
        &dependencies,
    )
    .unwrap();
    assert!(compatible_diagnostics.is_empty());
}

#[test]
fn python3_recipe_is_compatible_with_locked_python_profile_alias() {
    let mut manifest = manifest();
    manifest.recipes[0].prerequisite_tool_ids = vec!["python3".into()];
    let available_tools = ["python3".into()].into();
    let profiles = vec![["python".into()].into()];

    assert!(
        incompatible_recipe_toolchain_profiles(&manifest, "web", &available_tools, &profiles,)
            .is_empty()
    );
}

fn manifest() -> ProjectCapabilityManifestV1 {
    let task_id = "task-1".to_owned();
    let web_target = AppTargetV1 {
        target_id: "web".into(),
        owning_task_id: task_id.clone(),
        repository_id: "repo-1".into(),
        app_surface: AppSurface::Web,
        project_relative_root: "apps/web".into(),
        build_recipe_ref: Some("web-build".into()),
        test_recipe_refs: vec!["web-test".into()],
        start_recipe_ref: Some("web-start".into()),
        readiness_probe_ref: Some("health".into()),
        artifact_spec_ref: Some("web-artifacts".into()),
        fixture_ref: None,
        required_dependencies: vec!["npm:fixture".into()],
        supported_driver_ids: vec!["web-driver".into()],
        port_needs: vec![8080],
        environment_policy_ref: "default-deny".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
    let unsupported_target = AppTargetV1 {
        target_id: "unsupported".into(),
        owning_task_id: task_id,
        repository_id: "repo-1".into(),
        app_surface: AppSurface::UnsupportedTarget,
        project_relative_root: "devices/device".into(),
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
        capability_state: TargetCapabilityState::UnsupportedPlatform,
    };
    ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: "workspace-1".into(),
        repository_id: "repo-1".into(),
        source_sha: "a".repeat(40),
        detected_languages: vec!["JavaScript".into()],
        frameworks: vec!["Node.js".into()],
        candidate_targets: vec![web_target, unsupported_target],
        selected_target_id: Some("web".into()),
        target_capabilities: vec![
            TargetCapabilityState::ReadyButUntested,
            TargetCapabilityState::UnsupportedPlatform,
        ],
        recipes: ["web-build", "web-test", "web-start"]
            .into_iter()
            .map(recipe)
            .collect(),
        readiness_probes: [(
            "health".into(),
            ReadinessProbeV1::HttpOnApprovedPort {
                port: 8080,
                path: "/health".into(),
                expected_status: 200,
            },
        )]
        .into(),
        artifact_specs: [("web-artifacts".into(), vec!["dist/index.html".into()])].into(),
        confidence: 80,
        evidence_refs: vec!["apps/web/package.json".into()],
        user_overrides: Vec::new(),
        revision: 1,
    }
}

fn recipe(recipe_id: &str) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "node".into(),
        argv: vec!["--version".into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 30,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["node".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:projection-test".into()],
    }
}
