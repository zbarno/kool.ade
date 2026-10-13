use super::super::*;
use crate::artifacts::planning_store::{PlanningStore, StoreError};

struct Fixture(std::path::PathBuf, PlanningStore);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("koolade-runtime-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &root);
        Self(root, store)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn manifest_saves_with_revision_fence_and_loads_from_the_planning_store() {
    let fixture = Fixture::new();
    let runtime = ProjectRuntimeStore::new(&fixture.1);
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
    let project = fixture.0.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("pyproject.toml"),
        "[project]\ndependencies=['fastapi']\n",
    )
    .unwrap();
    let runtime = ProjectRuntimeStore::new(&fixture.1);
    let context = DiscoveryContext {
        workspace_id: "workspace-1".into(),
        repository_id: "repo-1".into(),
        owning_task_id: "task-1".into(),
        source_sha: "c".repeat(40),
    };
    let mut manifest = runtime.discover_and_save(&project, &context, 0).unwrap();
    assert_eq!(manifest.revision, 1);
    assert_eq!(manifest.detected_languages, ["Python"]);
    let target_id = manifest.candidate_targets[0].target_id.clone();
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
        evidence_refs: vec!["user-approval:approval-1".into()],
    };
    manifest = runtime
        .approve_recipe_and_save(&target_id, RuntimeOperation::Build, recipe, 1)
        .unwrap();
    assert_eq!(manifest.revision, 2);
    assert_eq!(runtime.load_manifest().unwrap(), Some(manifest.clone()));
    assert!(matches!(
        runtime.approve_recipe_and_save(
            &target_id,
            RuntimeOperation::Build,
            ExecutionRecipeV1 {
                recipe_id: "another-build".into(),
                ..manifest.user_overrides[0].clone()
            },
            1,
        ),
        Err(StoreError::StaleRecordRevision { .. })
    ));
}

#[test]
fn multiple_discovered_targets_require_explicit_selection() {
    let fixture = Fixture::new();
    let project = fixture.0.join("polyglot");
    std::fs::create_dir_all(project.join("apps/web")).unwrap();
    std::fs::create_dir_all(project.join("services/api")).unwrap();
    std::fs::write(project.join("apps/web/package.json"), r#"{"name":"web"}"#).unwrap();
    std::fs::write(
        project.join("services/api/pyproject.toml"),
        "[project]\nname='api'\n",
    )
    .unwrap();
    let runtime = ProjectRuntimeStore::new(&fixture.1);
    let context = DiscoveryContext {
        workspace_id: "workspace-polyglot".into(),
        repository_id: "repo-polyglot".into(),
        owning_task_id: "task-polyglot".into(),
        source_sha: "d".repeat(40),
    };
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
    let project = fixture.0.join("web");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("package.json"),
        r#"{"name":"web","dependencies":{"vite":"1"}}"#,
    )
    .unwrap();
    let runtime = ProjectRuntimeStore::new(&fixture.1);
    let context = DiscoveryContext {
        workspace_id: "workspace-web".into(),
        repository_id: "repo-web".into(),
        owning_task_id: "task-web".into(),
        source_sha: "e".repeat(40),
    };
    let manifest = runtime.discover_and_save(&project, &context, 0).unwrap();
    let target_id = manifest.candidate_targets[0].target_id.clone();
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
        evidence_refs: vec!["user-approval:approved-start".into()],
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

#[test]
fn receipt_records_are_append_only_and_task_source_pinned() {
    let fixture = Fixture::new();
    let runtime = ProjectRuntimeStore::new(&fixture.1);
    let receipt_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let receipt = RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: receipt_id.clone(),
        operation: RuntimeOperation::Build,
        task_id: "task-001".into(),
        target_id: "web".into(),
        source_sha: "b".repeat(40),
        recipe_id: Some("build-web".into()),
        state: RuntimeReceiptState::Passed,
        exit_code: Some(0),
        artifact_hashes: Default::default(),
        log_reference: Some("log-001".into()),
        observed_at_unix_ms: 1,
    };
    runtime.append_receipt(&receipt).unwrap();
    assert_eq!(
        runtime.load_receipt(&receipt_id).unwrap(),
        Some(receipt.clone())
    );
    let other_task = RuntimeReceiptV1 {
        receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        task_id: "task-002".into(),
        ..receipt.clone()
    };
    runtime.append_receipt(&other_task).unwrap();
    assert_eq!(
        runtime.receipts_for_task("task-001").unwrap(),
        vec![receipt.clone()]
    );
    assert!(runtime.append_receipt(&receipt).is_err());
}

#[cfg(unix)]
#[test]
fn runtime_store_rejects_symlinked_state_paths() {
    let fixture = Fixture::new();
    let external = fixture.0.join("outside");
    std::fs::create_dir_all(&external).unwrap();
    std::fs::create_dir_all(&fixture.1.root).unwrap();
    let state = fixture.1.root.join("state");
    std::os::unix::fs::symlink(&external, &state).unwrap();
    let runtime = ProjectRuntimeStore::new(&fixture.1);
    assert!(matches!(
        runtime.load_manifest(),
        Err(StoreError::InvalidPath(_))
    ));
}
