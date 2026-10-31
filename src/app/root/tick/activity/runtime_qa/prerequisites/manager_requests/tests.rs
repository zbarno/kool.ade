use super::{queue_missing_recipe_toolchain_reviews, request_id, toolchain_request};

#[test]
fn missing_runtime_toolchain_becomes_a_non_executable_manager_request() {
    let source_sha = "a".repeat(40);
    let request = toolchain_request(
        "task-uid",
        "planning-item",
        "web-target",
        "workspace",
        &source_sha,
        "python",
    );

    assert_eq!(request.task_id, "planning-item");
    assert_eq!(request.need.package.as_deref(), Some("python"));
    assert_eq!(
        request.need.kind,
        crate::harness::DependencyKind::SystemTool
    );
    assert_eq!(
        request.need.ecosystem,
        crate::harness::PackageEcosystem::System
    );
    assert_eq!(
        request.status,
        crate::harness::DependencyRequestStatus::Pending
    );
    assert_eq!(
        request.decision,
        crate::harness::DependencyDecision::RequiresUserAuthorization
    );
    assert!(request.need.command.contains("approved runtime toolchain"));
    assert!(
        request
            .need
            .source
            .as_deref()
            .unwrap()
            .starts_with("runtime-capability://")
    );
    assert!(request.preparation.is_none());
    assert_eq!(
        request.id,
        request_id("task-uid", "web-target", "workspace", &source_sha, "python")
    );
    assert!(request.need.source.unwrap().contains(&source_sha));
}

/// Boundary: validated local recipe approval is projected into Man.ager's
/// durable task review queue. A request-constructor unit test cannot prove the
/// target/source fence or duplicate suppression across the project activity.
/// Uses temporary local stores only; no Docker, network, or paid service.
#[test]
fn missing_recipe_toolchain_is_source_bound_and_deduplicated_in_manager_queue() {
    use crate::{
        app::session::test_support::project_from,
        artifacts::task_docs::{TaskDocument, TaskMetadata},
        core::lifecycle::{TaskPhase, TaskRecord},
        project_runtime::{
            AppSurface, AppTargetV1, ExecutionRecipeV1, NetworkRequirement,
            ProjectCapabilityManifestV1, ProjectRuntimeStore, RecipeSource, RuntimeOperation,
            TargetCapabilityState,
        },
    };

    let root = tempfile::tempdir().unwrap();
    let mut project = project_from(root.path());
    let runtime = ProjectRuntimeStore::new(&project.state.planning_store);
    let workspace_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let _cleanup =
        crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&workspace_id);
    let batch_identity = crate::domain::ArtifactIdentity::new("BATCH-1", "Synthetic batch");
    let mut identity = crate::domain::ArtifactIdentity::new("TASK-1", "Synthetic app target");
    identity.parent_uid = Some(batch_identity.uid);
    let planning_item_uid = identity.uid.clone();
    let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let target = AppTargetV1 {
        target_id: "synthetic-web".into(),
        owning_task_id: planning_item_uid.clone(),
        repository_id: "synthetic-repo".into(),
        app_surface: AppSurface::Web,
        project_relative_root: "apps/site".into(),
        build_recipe_ref: Some("site-build".into()),
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
    let approval_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "site-build".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "odin".into(),
        argv: vec!["build".into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: vec!["dist".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["odin".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![
            format!("user-approval:{approval_id}"),
            format!("user-recipe-approval:{approval_id}"),
        ],
    };
    let mut manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: workspace_id.clone(),
        repository_id: "synthetic-repo".into(),
        source_sha: "a".repeat(40),
        detected_languages: Vec::new(),
        frameworks: Vec::new(),
        candidate_targets: vec![target.clone()],
        selected_target_id: Some(target.target_id.clone()),
        target_capabilities: vec![TargetCapabilityState::ReadyButUntested],
        recipes: Vec::new(),
        readiness_probes: Default::default(),
        artifact_specs: Default::default(),
        confidence: 80,
        evidence_refs: Vec::new(),
        user_overrides: vec![recipe.clone()],
        revision: 0,
    };
    runtime.save_manifest(&manifest, 0).unwrap();
    manifest.revision = 1;
    let binding = ProjectRuntimeStore::user_recipe_approval_binding(
        &manifest,
        &target,
        RuntimeOperation::Build,
        &recipe,
    )
    .unwrap();
    crate::persistence::runtime_target_approvals::save(&binding).unwrap();

    let metadata = TaskMetadata::new(&identity, "synthetic-repo", Vec::new()).unwrap();
    project.task_documents.push(TaskDocument {
        path: "planning/tasks/TASK-1.md".into(),
        title: "Synthetic app target".into(),
        text: String::new(),
        identity: None,
        metadata: Some(metadata),
        task_state: None,
        metadata_error: None,
    });
    let task = TaskRecord {
        schema_version: 1,
        task_uid,
        feature_uid: "feature".into(),
        planning_item_uid: Some(planning_item_uid),
        task_spec_sha256: None,
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        approved_requirement_ids: Vec::new(),
        phase: TaskPhase::AwaitingRuntimeQa,
        revision: 1,
        approved_plan_sha256: "b".repeat(64),
        implementation_sha256: None,
        reviewed_code_head_sha: None,
        reviewed_code_subject_sha256: None,
        code_review: None,
        review_history: Vec::new(),
        repair_attempts: 0,
        runtime_qa: None,
        qa_execution: None,
        spec_reconciliation: None,
        spec_reconciliation_blocker: None,
        spec_reconciliation_proposal_sha256: None,
        spec_reconciliation_spec_sha256: None,
        spec_reconciliation_proposal_json: None,
        pr_work_item_uid: None,
    };

    let missing = ["odin".to_owned()];
    assert_eq!(
        queue_missing_recipe_toolchain_reviews(
            &mut project,
            &task,
            &target.target_id,
            &manifest.workspace_id,
            &manifest.source_sha,
            &["site-build".into()],
            &missing,
        ),
        missing
    );
    assert!(
        queue_missing_recipe_toolchain_reviews(
            &mut project,
            &task,
            &target.target_id,
            &manifest.workspace_id,
            &manifest.source_sha,
            &["site-build".into()],
            &["odin".into()],
        )
        .is_empty()
    );
    let requests = &project.activity.tasks["planning/tasks/TASK-1.md"].dependency_requests;
    assert_eq!(requests.len(), 1);
    let expected_source = format!(
        "runtime-capability://{}/{}@{}",
        manifest.workspace_id, target.target_id, manifest.source_sha
    );
    assert_eq!(
        requests[0].need.source.as_deref(),
        Some(expected_source.as_str())
    );
    assert_eq!(
        requests[0].status,
        crate::harness::DependencyRequestStatus::Pending
    );
    assert!(requests[0].preparation.is_none());
}
