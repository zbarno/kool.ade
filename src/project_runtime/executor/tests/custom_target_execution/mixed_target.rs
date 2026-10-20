use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    project_runtime::{
        AppSurface, DiscoveryContext, ExecutionRecipeV1, NetworkRequirement,
        ProjectRuntimeExecutor, ProjectRuntimeStore, ReadinessProbeV1, RecipeSource,
        RuntimeOperation,
    },
};

use super::setup::build;

fn adapter_recipe(recipe_id: &str, command: &str) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.into(),
        source: RecipeSource::TrustedAdapter,
        working_directory_relative: ".".into(),
        executable_id: "node".into(),
        argv: vec![command.into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["node".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: Vec::new(),
    }
}

#[test]
// Production boundary: a locally fingerprinted trusted adapter recipe must
// reach the selected verification backend for both build and start, while an
// edited recipe with the old provenance must be rejected before dispatch.
// Validator-only tests cannot prove the ExecutionSession policy reaches the
// backend. The fake backend is in-process; Docker, SSH, network, and host CLIs
// are not used, so setup cost is low.
fn verified_adapter_recipes_reach_backend_and_changed_provenance_is_denied() {
    let fixture = build();
    let project_root = tempfile::tempdir().unwrap();
    std::fs::write(project_root.path().join("package.json"), "{}\n").unwrap();
    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        project_root.path(),
        StoreMode::ManagedLocal,
    );
    let store = ProjectRuntimeStore::new(&planning);
    let context = DiscoveryContext {
        workspace_id: fixture.workspace_id.clone(),
        repository_id: "synthetic-repo".into(),
        owning_task_id: fixture.task_id.clone(),
        source_sha: fixture.source_sha.clone(),
    };
    let discovered = store
        .discover_and_save(project_root.path(), &context, 0)
        .unwrap();
    let target_id = discovered.candidate_targets[0].target_id.clone();
    let recipe = adapter_recipe("adapter-build", "build.js");
    store
        .add_trusted_adapter_recipe_and_save(
            &target_id,
            RuntimeOperation::Build,
            recipe,
            None,
            discovered.revision,
        )
        .unwrap();
    let start_recipe = adapter_recipe("adapter-start", "server.js");
    let after_start = store
        .add_trusted_adapter_recipe_and_save(
            &target_id,
            RuntimeOperation::Start,
            start_recipe,
            Some(ReadinessProbeV1::LocalServiceHealth {
                port: 3000,
                path: "/health".into(),
            }),
            2,
        )
        .unwrap();
    store
        .select_target_and_save(&fixture.task_id, &target_id, after_start.revision)
        .unwrap();

    let (_, selected) = ProjectRuntimeExecutor::new(&store)
        .validate_candidate(&fixture.session, &target_id, RuntimeOperation::Build)
        .unwrap();
    assert_eq!(selected.app_surface, AppSurface::HeadlessService);
    let executor = ProjectRuntimeExecutor::new(&store);
    executor
        .validate_candidate(&fixture.session, &target_id, RuntimeOperation::Start)
        .unwrap();
    let build = executor
        .execute(&fixture.session, &target_id, RuntimeOperation::Build)
        .unwrap();
    assert_eq!(
        build[0].state,
        crate::project_runtime::RuntimeReceiptState::Passed
    );
    let build_request = fixture.backend.request();
    assert_eq!(build_request.argv, ["node", "build.js"]);
    let build_policy = serde_json::to_value(&build_request.policy).unwrap();
    assert!(
        build_policy["approved_recipe_command"]["approval_ref"]
            .as_str()
            .is_some_and(|value| value.starts_with("trusted-adapter-provenance:"))
    );

    fixture.backend.clear_request();
    let _start = executor
        .execute(&fixture.session, &target_id, RuntimeOperation::Start)
        .unwrap();
    let start_request = fixture.backend.request();
    assert_eq!(start_request.argv, ["sh", "-s"]);
    assert!(
        start_request
            .stdin
            .as_deref()
            .unwrap()
            .contains("server.js")
    );

    let mut changed = store.load_manifest().unwrap().unwrap();
    changed
        .recipes
        .iter_mut()
        .find(|recipe| recipe.recipe_id == "adapter-build")
        .unwrap()
        .argv
        .push("changed.js".into());
    let expected_revision = changed.revision;
    store.save_manifest(&changed, expected_revision).unwrap();
    fixture.backend.clear_request();
    assert!(
        executor
            .execute(&fixture.session, &target_id, RuntimeOperation::Build)
            .is_err()
    );
    assert!(!fixture.backend.has_request());
}

#[test]
fn forged_start_recipe_cannot_be_laundered_by_saving_an_adapter_build() {
    let fixture = build();
    let project_root = tempfile::tempdir().unwrap();
    std::fs::write(project_root.path().join("package.json"), "{}\n").unwrap();
    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        project_root.path(),
        StoreMode::ManagedLocal,
    );
    let store = ProjectRuntimeStore::new(&planning);
    let context = DiscoveryContext {
        workspace_id: fixture.workspace_id.clone(),
        repository_id: "synthetic-repo".into(),
        owning_task_id: fixture.task_id.clone(),
        source_sha: fixture.source_sha.clone(),
    };
    let mut manifest = store
        .discover_and_save(project_root.path(), &context, 0)
        .unwrap();
    let target_id = manifest.candidate_targets[0].target_id.clone();
    let mut target = manifest.candidate_targets[0].clone();
    target.start_recipe_ref = Some("forged-start".into());
    manifest.candidate_targets[0] = target;
    manifest
        .recipes
        .push(adapter_recipe("forged-start", "arbitrary.js"));
    let expected_revision = manifest.revision;
    store.save_manifest(&manifest, expected_revision).unwrap();
    manifest.revision += 1;

    assert!(
        store
            .add_trusted_adapter_recipe_and_save(
                &target_id,
                RuntimeOperation::Build,
                adapter_recipe("adapter-build", "build.js"),
                None,
                manifest.revision,
            )
            .is_err()
    );
}
