use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    project_runtime::{
        AppSurface, DiscoveryContext, ExecutionRecipeV1, NetworkRequirement,
        ProjectRuntimeExecutor, ProjectRuntimeStore, RecipeSource, RuntimeOperation,
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
// Production boundary: validates that persisted trusted-adapter provenance is
// accepted by the ExecutionSession recipe authorization path and that changed
// or stale provenance is rejected. A hash helper unit test cannot exercise
// that execution boundary. Uses the local planning store and in-memory session;
// no Docker, SSH, network, or credentials, with low setup cost.
fn trusted_adapter_recipe_authorization_requires_current_local_provenance() {
    let fixture = build();
    let project_root = tempfile::tempdir().unwrap();
    let target_root = project_root.path().join("apps/web");
    std::fs::create_dir_all(&target_root).unwrap();
    std::fs::write(target_root.join("package.json"), "{}\n").unwrap();
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
    let manifest = store
        .discover_and_save(project_root.path(), &context, 0)
        .unwrap();
    let target_id = manifest.candidate_targets[0].target_id.clone();
    assert_eq!(
        manifest.candidate_targets[0].project_relative_root,
        "apps/web"
    );
    let recipe = adapter_recipe("adapter-build", "build.js");
    let manifest = store
        .add_trusted_adapter_recipe_and_save(
            &target_id,
            RuntimeOperation::Build,
            recipe,
            manifest.revision,
        )
        .unwrap();
    let manifest = store
        .select_target_and_save(&fixture.task_id, &target_id, manifest.revision)
        .unwrap();
    let recipe = &manifest.recipes[0];
    let mut workspace_recipe = recipe.clone();
    workspace_recipe.working_directory_relative =
        manifest.candidate_targets[0].project_relative_root.clone();

    assert!(
        fixture
            .session
            .verify_local_recipe_approval(
                &store,
                &target_id,
                &workspace_recipe,
                Some(&manifest.candidate_targets[0].project_relative_root),
            )
            .is_ok()
    );
    assert!(store.recipe_is_locally_approved(&manifest, &manifest.candidate_targets[0], recipe));

    let mut forged = workspace_recipe.clone();
    forged.argv.push("unapproved.js".into());
    assert!(
        fixture
            .session
            .verify_local_recipe_approval(
                &store,
                &target_id,
                &forged,
                Some(&manifest.candidate_targets[0].project_relative_root),
            )
            .is_err()
    );

    let mut ambiguous = manifest.clone();
    ambiguous.candidate_targets[0]
        .test_recipe_refs
        .push(recipe.recipe_id.clone());
    assert!(
        !store.recipe_is_locally_approved(&ambiguous, &ambiguous.candidate_targets[0], recipe,),
        "one recipe cannot be authorized for multiple runtime operations"
    );

    let mut stale = manifest.clone();
    stale.source_sha = "b".repeat(40);
    store.save_manifest(&stale, manifest.revision).unwrap();
    let stale = store.load_manifest().unwrap().unwrap();
    let stale_recipe = stale
        .recipes
        .iter()
        .find(|candidate| candidate.recipe_id == recipe.recipe_id)
        .unwrap();
    assert!(!store.recipe_is_locally_approved(&stale, &stale.candidate_targets[0], stale_recipe));
    assert!(
        fixture
            .session
            .verify_local_recipe_approval(
                &store,
                &target_id,
                stale_recipe,
                Some(&stale.candidate_targets[0].project_relative_root),
            )
            .is_err()
    );
}

#[test]
fn custom_origin_does_not_block_a_separate_discovered_target_with_verified_adapter_recipe() {
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
            discovered.revision,
        )
        .unwrap();
    let start_recipe = adapter_recipe("adapter-start", "server.js");
    let after_start = store
        .add_trusted_adapter_recipe_and_save(&target_id, RuntimeOperation::Start, start_recipe, 2)
        .unwrap();
    store
        .select_target_and_save(&fixture.task_id, &target_id, after_start.revision)
        .unwrap();

    let (_, selected) = ProjectRuntimeExecutor::new(&store)
        .validate_candidate(&fixture.session, &target_id, RuntimeOperation::Build)
        .unwrap();
    assert_eq!(selected.app_surface, AppSurface::HeadlessService);
    ProjectRuntimeExecutor::new(&store)
        .validate_candidate(&fixture.session, &target_id, RuntimeOperation::Start)
        .unwrap();
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
                manifest.revision,
            )
            .is_err()
    );
}
