use super::super::*;
use crate::artifacts::planning_store::{PlanningStore, StoreError};

#[path = "store/compatibility.rs"]
mod compatibility;
#[path = "store/receipt_records.rs"]
mod receipt_records;

struct Fixture {
    root: std::path::PathBuf,
    home_root: std::path::PathBuf,
    store: PlanningStore,
    _home_guard: crate::test_support::KooladeHomeGuard,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("koolade-runtime-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(&root)
                .status()
                .expect("git must be available for the project runtime fixture")
                .success()
        );
        let home_root =
            std::env::temp_dir().join(format!("koolade-runtime-home-{}", uuid::Uuid::new_v4()));
        let home = crate::test_support::KooladeHomeGuard::set(&home_root);
        let store = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &root);
        Self {
            root,
            home_root,
            store,
            _home_guard: home,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
        let _ = std::fs::remove_dir_all(&self.home_root);
    }
}

#[test]
fn manifest_saves_with_revision_fence_and_loads_from_the_planning_store() {
    let fixture = Fixture::new();
    let runtime = ProjectRuntimeStore::new(&fixture.store);
    let manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: uuid::Uuid::new_v4().to_string(),
        repository_id: "repo-main".into(),
        source_sha: "a".repeat(40),
        detected_languages: vec!["Python".into()],
        frameworks: Vec::new(),
        candidate_targets: Vec::new(),
        selected_target_id: None,
        target_capabilities: Vec::new(),
        recipes: Vec::new(),
        readiness_probes: Default::default(),
        artifact_specs: Default::default(),
        confidence: 50,
        evidence_refs: vec!["pyproject.toml".into()],
        user_overrides: Vec::new(),
        revision: 0,
    };
    runtime.save_manifest(&manifest, 0).unwrap();
    assert_eq!(runtime.load_manifest().unwrap().unwrap().revision, 1);
    assert!(matches!(
        runtime.save_manifest(&manifest, 0),
        Err(StoreError::StaleRecordRevision { .. })
    ));
}

#[test]
fn discovery_and_user_recipe_approval_are_persisted_with_manifest_cas() {
    let fixture = Fixture::new();
    let project = fixture.root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("pyproject.toml"),
        "[project]\ndependencies=['fastapi']\n",
    )
    .unwrap();
    let runtime = ProjectRuntimeStore::new(&fixture.store);
    let context = DiscoveryContext {
        workspace_id: uuid::Uuid::new_v4().to_string(),
        repository_id: "repo-1".into(),
        owning_task_id: uuid::Uuid::new_v4().to_string(),
        source_sha: "c".repeat(40),
    };
    let _approval_cleanup =
        crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&context.workspace_id);
    let mut manifest = runtime.discover_and_save(&project, &context, 0).unwrap();
    assert_eq!(manifest.revision, 1);
    assert_eq!(manifest.detected_languages, ["Python"]);
    let target_id = manifest.candidate_targets[0].target_id.clone();
    let approval_proof = crate::ui::layout::RuntimeTargetApprovalProof::for_test();
    let approval_id = approval_proof.receipt_uid();
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "python-build".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "python".into(),
        argv: vec!["-m".into(), "compileall".into(), ".".into()],
        allowed_env_keys: vec![],
        allowed_output_paths: vec!["__pycache__".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["python".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![format!("user-approval:{approval_id}")],
    };
    manifest = runtime
        .approve_recipe_and_save(
            &target_id,
            RuntimeOperation::Build,
            recipe,
            approval_proof,
            1,
        )
        .unwrap();
    assert_eq!(manifest.revision, 2);
    assert_eq!(runtime.load_manifest().unwrap(), Some(manifest.clone()));
    let target = &manifest.candidate_targets[0];
    let recipe = &manifest.user_overrides[0];
    assert!(runtime.recipe_is_locally_approved(&manifest, target, recipe));
    let mut forged = manifest.clone();
    forged.user_overrides[0].evidence_refs = vec!["user-approval:made-up".into()];
    assert!(!runtime.recipe_is_locally_approved(
        &forged,
        &forged.candidate_targets[0],
        &forged.user_overrides[0]
    ));
    let mut changed_recipe = recipe.clone();
    changed_recipe.argv.push("--changed".into());
    assert!(!runtime.recipe_is_locally_approved(&manifest, target, &changed_recipe));
    let mut changed_manifest = manifest.clone();
    changed_manifest.source_sha = "f".repeat(40);
    assert!(!runtime.recipe_is_locally_approved(
        &changed_manifest,
        &changed_manifest.candidate_targets[0],
        &changed_manifest.user_overrides[0]
    ));
    let mut changed_target = manifest.candidate_targets[0].clone();
    changed_target.project_relative_root = "private".into();
    assert!(!runtime.recipe_is_locally_approved(
        &manifest,
        &changed_target,
        &manifest.user_overrides[0]
    ));
    let stale_proof = crate::ui::layout::RuntimeTargetApprovalProof::for_test();
    let stale_id = stale_proof.receipt_uid();
    let mut stale_recipe = manifest.user_overrides[0].clone();
    stale_recipe.recipe_id = "another-build".into();
    stale_recipe.evidence_refs.retain(|reference| {
        !reference.starts_with("user-approval:") && !reference.starts_with("user-recipe-approval:")
    });
    stale_recipe
        .evidence_refs
        .push(format!("user-approval:{stale_id}"));
    assert!(matches!(
        runtime.approve_recipe_and_save(
            &target_id,
            RuntimeOperation::Build,
            stale_recipe,
            stale_proof,
            1,
        ),
        Err(StoreError::StaleRecordRevision { .. })
    ));
}

#[test]
fn multiple_discovered_targets_require_explicit_selection() {
    let fixture = Fixture::new();
    let project = fixture.root.join("polyglot");
    std::fs::create_dir_all(project.join("apps/web")).unwrap();
    std::fs::create_dir_all(project.join("services/api")).unwrap();
    std::fs::write(project.join("apps/web/package.json"), r#"{"name":"web"}"#).unwrap();
    std::fs::write(
        project.join("services/api/pyproject.toml"),
        "[project]\nname='api'\n",
    )
    .unwrap();
    let runtime = ProjectRuntimeStore::new(&fixture.store);
    let context = DiscoveryContext {
        workspace_id: uuid::Uuid::new_v4().to_string(),
        repository_id: "repo-polyglot".into(),
        owning_task_id: uuid::Uuid::new_v4().to_string(),
        source_sha: "d".repeat(40),
    };
    let _approval_cleanup =
        crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&context.workspace_id);
    let manifest = runtime.discover_and_save(&project, &context, 0).unwrap();
    assert_eq!(manifest.candidate_targets.len(), 2);
    assert!(manifest.selected_target_id.is_none());
    assert!(
        manifest
            .candidate_targets
            .iter()
            .all(|target| !manifest.is_active_target(&context.owning_task_id, &target.target_id))
    );

    let selected = manifest.candidate_targets[0].target_id.clone();
    let manifest = runtime
        .select_target_and_save(&context.owning_task_id, &selected, manifest.revision)
        .unwrap();
    assert_eq!(
        manifest.selected_target_id.as_deref(),
        Some(selected.as_str())
    );
    assert!(manifest.is_active_target(&context.owning_task_id, &selected));
    assert!(
        manifest
            .candidate_targets
            .iter()
            .filter(|target| target.target_id != selected)
            .all(|target| !manifest.is_active_target(&context.owning_task_id, &target.target_id))
    );
    assert!(matches!(
        runtime.select_target_and_save(&context.owning_task_id, &selected, 1),
        Err(StoreError::StaleRecordRevision { .. })
    ));
}

#[test]
fn approved_start_recipe_and_readiness_probe_commit_together() {
    let fixture = Fixture::new();
    let project = fixture.root.join("web");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("package.json"),
        r#"{"name":"web","dependencies":{"vite":"1"}}"#,
    )
    .unwrap();
    let runtime = ProjectRuntimeStore::new(&fixture.store);
    let context = DiscoveryContext {
        workspace_id: uuid::Uuid::new_v4().to_string(),
        repository_id: "repo-web".into(),
        owning_task_id: uuid::Uuid::new_v4().to_string(),
        source_sha: "e".repeat(40),
    };
    let _approval_cleanup =
        crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&context.workspace_id);
    let manifest = runtime.discover_and_save(&project, &context, 0).unwrap();
    let target_id = manifest.candidate_targets[0].target_id.clone();
    let approval_proof = crate::ui::layout::RuntimeTargetApprovalProof::for_test();
    let approval_id = approval_proof.receipt_uid();
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "web-start".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "node".into(),
        argv: vec!["server.js".into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["node".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![format!("user-approval:{approval_id}")],
    };
    let saved = runtime
        .approve_recipe_with_readiness_and_save(
            &target_id,
            RuntimeOperation::Start,
            recipe,
            Some(ReadinessProbeV1::HttpOnApprovedPort {
                port: 3000,
                path: "/health".into(),
                expected_status: 200,
            }),
            approval_proof,
            manifest.revision,
        )
        .unwrap();
    let target = &saved.candidate_targets[0];
    assert_eq!(target.start_recipe_ref.as_deref(), Some("web-start"));
    assert_eq!(target.port_needs, [3000]);
    let probe_id = target.readiness_probe_ref.as_ref().unwrap();
    assert_eq!(
        saved.readiness_probes.get(probe_id),
        Some(&ReadinessProbeV1::HttpOnApprovedPort {
            port: 3000,
            path: "/health".into(),
            expected_status: 200,
        })
    );
}
