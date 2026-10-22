use super::super::super::request::PreparedRequest;
use super::run;
use crate::{
    artifacts::planning_store::PlanningStore,
    execution::contract::WorkspaceRef,
    project_runtime::{
        AppSurface, AppTargetV1, ExecutionRecipeV1, NetworkRequirement,
        ProjectCapabilityManifestV1, ProjectRuntimeStore, ReadinessProbeV1, RecipeSource,
        RuntimeOperation, RuntimeReceiptState, RuntimeReceiptV1, TargetCapabilityState,
    },
    runtime_qa::{QaBoundedLimitsV1, QaPlatform, QaRunRequestV1},
};
use sha2::Digest as _;
use std::sync::{Arc, atomic::AtomicBool};

#[path = "tests/platform.rs"]
mod platform;

struct UnusedHarness;

impl crate::harness::AiHarness for UnusedHarness {
    fn label(&self) -> String {
        "unused worker test harness".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        unreachable!("dispatch validation must reject before harness access")
    }

    fn execute(
        &self,
        _: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        unreachable!("dispatch validation must reject before harness access")
    }
}

fn approved_recipe(recipe_id: &str, args: &[&str]) -> ExecutionRecipeV1 {
    let approval_id = uuid::Uuid::new_v4().hyphenated().to_string();
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "node".into(),
        argv: args.iter().map(|arg| (*arg).into()).collect(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: vec!["dist".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["node".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![
            format!("user-approval:{approval_id}"),
            format!("user-recipe-approval:{approval_id}"),
        ],
    }
}

#[test]
// Production boundary: the persisted QA worker rechecks approval and target
// selection immediately before dispatch. A request-builder unit test cannot
// prove revoked state is observed at execution time. It uses a temporary store
// and unreachable harness only; no Docker, SSH, network, or credentials, with
// low in-process setup cost.
fn worker_rejects_revoked_prepared_start_after_selection_changes_to_new_target() {
    let root = tempfile::tempdir().unwrap();
    let planning = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), root.path());
    let runtime = ProjectRuntimeStore::new(&planning);
    let workspace_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let _approval_cleanup =
        crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&workspace_id);
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let target_a = AppTargetV1 {
        target_id: "qa-target".into(),
        owning_task_id: task_id.clone(),
        repository_id: "qa-repo".into(),
        app_surface: AppSurface::Web,
        project_relative_root: "apps/web".into(),
        build_recipe_ref: Some("qa-build".into()),
        test_recipe_refs: Vec::new(),
        start_recipe_ref: Some("qa-start-a".into()),
        readiness_probe_ref: Some("qa-ready-a".into()),
        artifact_spec_ref: Some("qa-artifacts-a".into()),
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: vec![3000],
        environment_policy_ref: "default-deny".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
    let build = approved_recipe("qa-build", &["build"]);
    let start_a = approved_recipe("qa-start-a", &["start-a"]);
    let mut manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: workspace_id.clone(),
        repository_id: "qa-repo".into(),
        source_sha: "a".repeat(40),
        detected_languages: Vec::new(),
        frameworks: Vec::new(),
        candidate_targets: vec![target_a.clone()],
        selected_target_id: Some(target_a.target_id.clone()),
        target_capabilities: vec![TargetCapabilityState::ReadyButUntested],
        recipes: Vec::new(),
        readiness_probes: [(
            "qa-ready-a".into(),
            ReadinessProbeV1::HttpOnApprovedPort {
                port: 3000,
                path: "/health".into(),
                expected_status: 200,
            },
        )]
        .into_iter()
        .collect(),
        artifact_specs: [("qa-artifacts-a".into(), vec!["dist/a.html".into()])]
            .into_iter()
            .collect(),
        confidence: 80,
        evidence_refs: Vec::new(),
        user_overrides: vec![build.clone(), start_a.clone()],
        revision: 0,
    };
    runtime.save_manifest(&manifest, 0).unwrap();
    manifest.revision = 1;
    for (operation, recipe) in [
        (RuntimeOperation::Build, &build),
        (RuntimeOperation::Start, &start_a),
    ] {
        let binding = ProjectRuntimeStore::user_recipe_approval_binding(
            &manifest, &target_a, operation, recipe,
        )
        .unwrap();
        crate::persistence::runtime_target_approvals::save(&binding).unwrap();
    }

    let mut prepared_start = start_a.clone();
    prepared_start.working_directory_relative = "apps/web".into();
    let target_b = AppTargetV1 {
        target_id: "qa-target-b".into(),
        build_recipe_ref: Some("qa-build-b".into()),
        start_recipe_ref: Some("qa-start-b".into()),
        readiness_probe_ref: Some("qa-ready-b".into()),
        artifact_spec_ref: Some("qa-artifacts-b".into()),
        port_needs: vec![3001],
        ..target_a.clone()
    };
    let build_b = approved_recipe("qa-build-b", &["build-b"]);
    let start_b = approved_recipe("qa-start-b", &["start-b"]);
    manifest.candidate_targets = vec![target_a.clone(), target_b.clone()];
    manifest.target_capabilities = vec![
        TargetCapabilityState::ReadyButUntested,
        TargetCapabilityState::ReadyButUntested,
    ];
    manifest.selected_target_id = Some(target_b.target_id.clone());
    manifest.user_overrides.push(build_b.clone());
    manifest.user_overrides.push(start_b.clone());
    manifest.readiness_probes.insert(
        "qa-ready-b".into(),
        ReadinessProbeV1::HttpOnApprovedPort {
            port: 3001,
            path: "/ready".into(),
            expected_status: 200,
        },
    );
    manifest
        .artifact_specs
        .insert("qa-artifacts-b".into(), vec!["dist/b.html".into()]);
    runtime.save_manifest(&manifest, 1).unwrap();
    let current = runtime.load_manifest().unwrap().unwrap();
    for (operation, recipe) in [
        (RuntimeOperation::Build, &build_b),
        (RuntimeOperation::Start, &start_b),
    ] {
        let binding = ProjectRuntimeStore::user_recipe_approval_binding(
            &current, &target_b, operation, recipe,
        )
        .unwrap();
        crate::persistence::runtime_target_approvals::save(&binding).unwrap();
    }
    runtime
        .verify_execution_provenance(&current, &target_b, RuntimeOperation::Start)
        .expect("the replacement Start is approved and current");
    let stale_binding = ProjectRuntimeStore::user_recipe_approval_binding(
        &current,
        &target_a,
        RuntimeOperation::Start,
        &start_a,
    )
    .unwrap();
    crate::persistence::runtime_target_approvals::revoke(&stale_binding).unwrap();

    let workspace = WorkspaceRef {
        project_id: workspace_id,
        task_id: task_id.clone(),
        source_sha: current.source_sha.clone(),
    };
    let recipe_sha = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&prepared_start).unwrap())
    );
    let request = QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        parent_task_uid: task_id.clone(),
        planning_item_uid: None,
        feature_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        code_head_sha: current.source_sha.clone(),
        build_artifact_sha256: "b".repeat(64),
        complete_diff_hash: "c".repeat(64),
        review_subject_sha256: "d".repeat(64),
        approved_spec_hash: "e".repeat(64),
        requirement_ids: vec!["requirement-1".into()],
        acceptance_criteria: vec!["The app starts".into()],
        relevant_platforms: vec![QaPlatform::Web],
        startup_recipe_ref: start_a.recipe_id.clone(),
        startup_recipe_sha256: recipe_sha,
        execution_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        driver_profile_version: "test-driver".into(),
        test_data_fixture_ref: None,
        dependency_cache_refs: Vec::new(),
        bounded_limits: QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 1,
            max_total_actions: 1,
            timeout_seconds: 60,
            max_evidence_bytes: 1024,
            max_screenshots: 1,
        },
        author_run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        author_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
    };
    let prepared = PreparedRequest {
        request,
        task_clone: root.path().join("task-clone"),
        planning_store: planning,
        workspace,
        target: target_a,
        startup_recipe: prepared_start,
        build_receipt: RuntimeReceiptV1 {
            schema_version: 1,
            receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            operation: RuntimeOperation::Build,
            task_id,
            target_id: "qa-target".into(),
            source_sha: current.source_sha,
            recipe_id: Some("qa-build".into()),
            state: RuntimeReceiptState::Passed,
            exit_code: Some(0),
            artifact_hashes: Default::default(),
            prepared_dependency_ids: Vec::new(),
            log_reference: None,
            observed_at_unix_ms: 1,
        },
        artifact_paths: vec!["apps/web/dist/a.html".into()],
        readiness_probe: Some(ReadinessProbeV1::HttpOnApprovedPort {
            port: 3000,
            path: "/health".into(),
            expected_status: 200,
        }),
        task_routes: Default::default(),
    };
    let (_cancel_sender, cancel_receiver) = tokio::sync::watch::channel(false);
    let (progress_sender, _progress_receiver) = std::sync::mpsc::channel();
    let error = run(
        prepared,
        Vec::new(),
        Box::new(UnusedHarness),
        root.path().join("runtime-qa"),
        Arc::new(AtomicBool::new(false)),
        cancel_receiver,
        progress_sender,
    )
    .unwrap_err();
    assert!(error.contains("runtime target is no longer the active selection"));
}
