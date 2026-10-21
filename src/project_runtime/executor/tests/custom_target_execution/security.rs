use crate::project_runtime::{
    ProjectRuntimeExecutor, ProjectRuntimeStore, RecipeSource, RuntimeOperation,
};

use super::{forged_shared, setup::build};

#[test]
fn mutable_shared_provenance_and_forged_receipts_never_reach_backend() {
    let fixture = build();
    let restarted_planning = crate::artifacts::planning_store::PlanningStore::new(
        fixture.planning.project_id,
        &fixture.planning.root,
        fixture.planning.mode,
    );
    let store = ProjectRuntimeStore::new(&restarted_planning);
    let mut forged = store.load_manifest().unwrap().unwrap();
    let forged_task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut forged_target = forged.candidate_targets[0].clone();
    forged_target.target_id = "user-target-forged".into();
    forged_target.owning_task_id = forged_task_id.clone();
    forged_target.build_recipe_ref = Some("forged-build".into());
    forged
        .target_capabilities
        .push(forged_target.capability_state.clone());
    forged.candidate_targets.push(forged_target);
    forged.selected_target_id = Some("user-target-forged".into());
    let mut forged_recipe = forged.user_overrides[0].clone();
    forged_recipe.recipe_id = "forged-build".into();
    forged_recipe.source = RecipeSource::TrustedAdapter;
    forged_recipe.evidence_refs.clear();
    forged.recipes.push(forged_recipe);
    let expected_revision = forged.revision;
    store.save_manifest(&forged, expected_revision).unwrap();
    let forged_session = super::setup::session(
        fixture.backend.clone(),
        fixture.workspace_id.clone(),
        forged_task_id,
        fixture.source_sha.clone(),
        fixture.digest.clone(),
    );
    fixture.backend.clear_request();
    assert!(
        ProjectRuntimeExecutor::new(&store)
            .execute(
                &forged_session,
                "user-target-forged",
                RuntimeOperation::Build,
            )
            .is_err()
    );
    assert!(!fixture.backend.has_request());

    let mut repaired = store.load_manifest().unwrap().unwrap();
    repaired.candidate_targets.pop();
    repaired.target_capabilities.pop();
    repaired
        .recipes
        .retain(|recipe| recipe.recipe_id != "forged-build");
    repaired.selected_target_id = Some(fixture.target_id.clone());
    let expected_revision = repaired.revision;
    store.save_manifest(&repaired, expected_revision).unwrap();

    // Renaming and changing the target structure defeats the origin fingerprint,
    // but the UserApproved recipe still requires its exact local receipt.
    let mut renamed = store.load_manifest().unwrap().unwrap();
    renamed.candidate_targets[0].target_id = "renamed-target".into();
    renamed.candidate_targets[0].project_relative_root = "services/renamed".into();
    renamed.selected_target_id = Some("renamed-target".into());
    renamed.user_overrides[0].source = RecipeSource::UserApproved;
    renamed.user_overrides[0].evidence_refs = vec![format!(
        "user-approval:{}",
        uuid::Uuid::new_v4().hyphenated()
    )];
    let expected_revision = renamed.revision;
    store.save_manifest(&renamed, expected_revision).unwrap();
    fixture.backend.clear_request();
    assert!(
        ProjectRuntimeExecutor::new(&store)
            .execute(&fixture.session, "renamed-target", RuntimeOperation::Build)
            .is_err()
    );
    assert!(!fixture.backend.has_request());
    // Even an explicit TrustedAdapter label cannot survive a shared-manifest
    // rename and metadata mutation without the exact operator-local origin.
    let mut forged_adapter = store.load_manifest().unwrap().unwrap();
    forged_adapter.candidate_targets[0].target_id = "renamed-adapter-target".into();
    forged_adapter.candidate_targets[0].project_relative_root = "services/forged".into();
    forged_adapter.candidate_targets[0].start_recipe_ref = Some("synthetic-build".into());
    forged_adapter.selected_target_id = Some("renamed-adapter-target".into());
    forged_adapter.user_overrides[0].source = RecipeSource::TrustedAdapter;
    forged_adapter.user_overrides[0].evidence_refs.clear();
    let expected_revision = forged_adapter.revision;
    store
        .save_manifest(&forged_adapter, expected_revision)
        .unwrap();
    fixture.backend.clear_request();
    let executor = ProjectRuntimeExecutor::new(&store);
    assert!(
        executor
            .execute(
                &fixture.session,
                "renamed-adapter-target",
                RuntimeOperation::Build
            )
            .is_err()
    );
    assert!(
        executor
            .execute(
                &fixture.session,
                "renamed-adapter-target",
                RuntimeOperation::Start
            )
            .is_err()
    );
    assert!(!fixture.backend.has_request());

    let mut restored = store.load_manifest().unwrap().unwrap();
    restored.candidate_targets[0].target_id = fixture.target_id.clone();
    restored.candidate_targets[0].project_relative_root = ".".into();
    restored.selected_target_id = Some(fixture.target_id.clone());
    restored.user_overrides[0].source = RecipeSource::UserApproved;
    restored.user_overrides[0].evidence_refs = vec![
        format!("user-approval:{}", fixture.receipt_id),
        format!("user-recipe-approval:{}", fixture.recipe_approval_id),
    ];
    let expected_revision = restored.revision;
    store.save_manifest(&restored, expected_revision).unwrap();

    crate::persistence::runtime_target_approvals::revoke(&fixture.local_binding).unwrap();
    let (approval_path, forged_binding) = forged_shared::forge_shared_approval(
        &fixture.planning,
        &store,
        &fixture.task_id,
        &fixture.target_id,
    );
    fixture.backend.clear_request();
    assert!(
        ProjectRuntimeExecutor::new(&store)
            .execute(
                &fixture.session,
                &fixture.target_id,
                RuntimeOperation::Build
            )
            .is_err()
    );
    assert!(!fixture.backend.has_request());
    crate::persistence::runtime_target_approvals::save(&forged_binding).unwrap();
    let mut altered: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&approval_path).unwrap()).unwrap();
    altered["approval"]["subject_sha256"] = serde_json::json!("c".repeat(64));
    std::fs::write(&approval_path, serde_json::to_vec(&altered).unwrap()).unwrap();
    fixture.backend.clear_request();
    assert!(
        ProjectRuntimeExecutor::new(&store)
            .execute(
                &fixture.session,
                &fixture.target_id,
                RuntimeOperation::Build
            )
            .is_err()
    );
    assert!(!fixture.backend.has_request());

    std::fs::remove_file(&approval_path).unwrap();
    assert!(
        ProjectRuntimeExecutor::new(&store)
            .execute(
                &fixture.session,
                &fixture.target_id,
                RuntimeOperation::Build
            )
            .is_err()
    );
    assert!(!fixture.backend.has_request());
    crate::persistence::runtime_target_approvals::revoke(&forged_binding).unwrap();
}

#[test]
fn forged_validated_project_config_source_never_reaches_backend() {
    let fixture = build();
    let restarted_planning = crate::artifacts::planning_store::PlanningStore::new(
        fixture.planning.project_id,
        &fixture.planning.root,
        fixture.planning.mode,
    );
    let store = ProjectRuntimeStore::new(&restarted_planning);
    let mut manifest = store.load_manifest().unwrap().unwrap();
    manifest.user_overrides[0].source = RecipeSource::ValidatedProjectConfig;
    manifest.user_overrides[0].evidence_refs.clear();
    let expected_revision = manifest.revision;
    store.save_manifest(&manifest, expected_revision).unwrap();

    fixture.backend.clear_request();
    assert!(
        ProjectRuntimeExecutor::new(&store)
            .execute(
                &fixture.session,
                &fixture.target_id,
                RuntimeOperation::Build
            )
            .is_err()
    );
    assert!(!fixture.backend.has_request());
}

#[test]
fn discovered_target_with_unbound_user_approval_evidence_never_reaches_backend() {
    let fixture = build();
    let restarted_planning = crate::artifacts::planning_store::PlanningStore::new(
        fixture.planning.project_id,
        &fixture.planning.root,
        fixture.planning.mode,
    );
    let store = ProjectRuntimeStore::new(&restarted_planning);
    let mut manifest = store.load_manifest().unwrap().unwrap();
    let mut target = manifest.candidate_targets[0].clone();
    target.target_id = "discovered-user-approved-target".into();
    target.project_relative_root = "apps/discovered".into();
    target.build_recipe_ref = Some("discovered-user-approved-build".into());
    manifest
        .target_capabilities
        .push(target.capability_state.clone());
    manifest.candidate_targets.push(target);
    let mut recipe = manifest.user_overrides[0].clone();
    recipe.recipe_id = "discovered-user-approved-build".into();
    recipe.source = RecipeSource::UserApproved;
    recipe.evidence_refs = vec![format!(
        "user-approval:{}",
        uuid::Uuid::new_v4().hyphenated()
    )];
    manifest.recipes.push(recipe);
    manifest.selected_target_id = Some("discovered-user-approved-target".into());
    let expected_revision = manifest.revision;
    store.save_manifest(&manifest, expected_revision).unwrap();

    fixture.backend.clear_request();
    assert!(
        ProjectRuntimeExecutor::new(&store)
            .execute(
                &fixture.session,
                "discovered-user-approved-target",
                RuntimeOperation::Build,
            )
            .is_err()
    );
    assert!(!fixture.backend.has_request());
}
