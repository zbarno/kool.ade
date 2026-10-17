use super::*;
use crate::{
    project_runtime::{ExecutionRecipeV1, NetworkRequirement, ReadinessProbeV1, RecipeSource},
    project_runtime_capability::CapabilityInventory,
};

fn recipe(id: &str, tool: &str) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: tool.into(),
        argv: vec!["--version".into()],
        allowed_env_keys: vec![],
        allowed_output_paths: vec![],
        timeout_seconds: 30,
        expected_exit_codes: vec![0],
        expected_artifacts: vec![],
        cache_scope: None,
        prerequisite_tool_ids: vec![tool.into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:adapter-fixture".into()],
    }
}

fn manifest(
    language: &str,
    framework: &str,
    workspace: &str,
    task: &str,
) -> ProjectCapabilityManifestV1 {
    let build = format!("{language}-build");
    let test = format!("{language}-test");
    let target = crate::project_runtime::AppTargetV1 {
        target_id: framework.into(),
        owning_task_id: task.into(),
        repository_id: "repo".into(),
        app_surface: AppSurface::Web,
        project_relative_root: format!("apps/{framework}"),
        build_recipe_ref: Some(build.clone()),
        test_recipe_refs: vec![test.clone()],
        start_recipe_ref: Some(format!("{language}-start")),
        readiness_probe_ref: Some("health".into()),
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: vec![],
        supported_driver_ids: vec!["browser".into()],
        port_needs: vec![8080],
        environment_policy_ref: "runtime-default".into(),
        capability_state: TargetCapabilityState::Ready,
    };
    ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: workspace.into(),
        repository_id: "repo".into(),
        source_sha: "a".repeat(40),
        detected_languages: vec![language.into()],
        frameworks: vec![framework.into()],
        candidate_targets: vec![target],
        selected_target_id: None,
        target_capabilities: vec![TargetCapabilityState::Ready],
        recipes: vec![
            recipe(&build, "node"),
            recipe(&test, "node"),
            recipe(&format!("{language}-start"), "node"),
        ],
        readiness_probes: [(
            "health".into(),
            ReadinessProbeV1::LocalServiceHealth {
                port: 8080,
                path: "/health".into(),
            },
        )]
        .into(),
        artifact_specs: Default::default(),
        confidence: 80,
        evidence_refs: vec!["project metadata".into()],
        user_overrides: vec![],
        revision: 1,
    }
}

#[test]
fn persisted_targets_resolve_by_surface_for_python_and_typescript() {
    let task = uuid::Uuid::new_v4().to_string();
    let workspace = uuid::Uuid::new_v4().to_string();
    let python = manifest("Python", "FastAPI", &workspace, &task);
    let typescript = manifest("TypeScript", "Next.js", &workspace, &task);
    let tools = ["node".to_owned()].into();
    let dependencies = BTreeSet::new();
    let drivers = ["browser".to_owned()].into();
    let inventory = ApprovedCapabilityInventory {
        manifest: &python,
        available_tool_ids: &tools,
        available_dependency_ids: &dependencies,
        available_driver_ids: &drivers,
    };
    let py = assess_manifest_target(&python, "FastAPI", &task, &inventory).unwrap();
    let ts_inventory = ApprovedCapabilityInventory {
        manifest: &typescript,
        ..inventory
    };
    let ts = assess_manifest_target(&typescript, "Next.js", &task, &ts_inventory).unwrap();
    assert_eq!(py.report.selected_driver_id, ts.report.selected_driver_id);
    assert_eq!(py.report.qa_applicability, QaApplicability::Visual);
    assert_eq!(ts.report.qa_applicability, QaApplicability::Visual);
    assert_eq!(py.capability_state, TargetCapabilityState::Ready);
    assert_eq!(ts.capability_state, TargetCapabilityState::Ready);
}

#[test]
fn selected_target_cannot_be_guessed_from_another_task_or_manifest() {
    let task = uuid::Uuid::new_v4().to_string();
    let manifest = manifest(
        "Python",
        "FastAPI",
        &uuid::Uuid::new_v4().to_string(),
        &task,
    );
    let tools = ["node".to_owned()].into();
    let inventory = ApprovedCapabilityInventory {
        manifest: &manifest,
        available_tool_ids: &tools,
        available_dependency_ids: &BTreeSet::new(),
        available_driver_ids: &BTreeSet::new(),
    };
    assert!(assess_manifest_target(&manifest, "FastAPI", "different-task", &inventory).is_err());
}

#[test]
fn persisted_trusted_adapter_label_does_not_authorize_a_recipe() {
    let mut manifest = manifest(
        "Python",
        "FastAPI",
        &uuid::Uuid::new_v4().to_string(),
        &uuid::Uuid::new_v4().to_string(),
    );
    manifest.recipes[0].source = RecipeSource::TrustedAdapter;
    manifest.recipes[0].evidence_refs.clear();
    let tools = ["node".to_owned()].into();
    let inventory = ApprovedCapabilityInventory {
        manifest: &manifest,
        available_tool_ids: &tools,
        available_dependency_ids: &BTreeSet::new(),
        available_driver_ids: &BTreeSet::new(),
    };
    assert!(!inventory.recipe("Python-build").unwrap().authorized);
}
