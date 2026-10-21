use super::{
    latest_start_receipt, latest_successful_build_receipt, task_spec::acceptance_criteria,
    workspace_target_path,
};
use crate::project_runtime::{RuntimeOperation, RuntimeReceiptState, RuntimeReceiptV1};

#[test]
fn qa_dispatch_rechecks_local_build_and_start_approval_before_launch() {
    use crate::{
        artifacts::planning_store::PlanningStore,
        project_runtime::{
            AppSurface, AppTargetV1, ExecutionRecipeV1, NetworkRequirement,
            ProjectCapabilityManifestV1, ProjectRuntimeStore, ReadinessProbeV1, RecipeSource,
            TargetCapabilityState,
        },
    };

    let root = tempfile::tempdir().unwrap();
    let planning = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), root.path());
    let runtime = ProjectRuntimeStore::new(&planning);
    let workspace_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let _approval_cleanup =
        crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&workspace_id);
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let target = AppTargetV1 {
        target_id: "target-qa-provenance".into(),
        owning_task_id: task_id,
        repository_id: "synthetic-repo".into(),
        app_surface: AppSurface::Web,
        project_relative_root: "apps/web".into(),
        build_recipe_ref: Some("qa-build".into()),
        test_recipe_refs: Vec::new(),
        start_recipe_ref: Some("qa-start".into()),
        readiness_probe_ref: Some("qa-ready".into()),
        artifact_spec_ref: Some("qa-artifacts".into()),
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: vec![3000],
        environment_policy_ref: "default-deny".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
    let recipe = |recipe_id: &str, receipt_id: String, argv: Vec<&str>| ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "node".into(),
        argv: argv.into_iter().map(str::to_owned).collect(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: vec!["dist".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["node".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![
            format!("user-approval:{receipt_id}"),
            format!("user-recipe-approval:{receipt_id}"),
        ],
    };
    let build = recipe(
        "qa-build",
        uuid::Uuid::new_v4().hyphenated().to_string(),
        vec!["build"],
    );
    let start = recipe(
        "qa-start",
        uuid::Uuid::new_v4().hyphenated().to_string(),
        vec!["start"],
    );
    let readiness = ReadinessProbeV1::HttpOnApprovedPort {
        port: 3000,
        path: "/health".into(),
        expected_status: 200,
    };
    let mut manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id,
        repository_id: "synthetic-repo".into(),
        source_sha: "a".repeat(40),
        detected_languages: Vec::new(),
        frameworks: Vec::new(),
        candidate_targets: vec![target.clone()],
        selected_target_id: Some(target.target_id.clone()),
        target_capabilities: vec![TargetCapabilityState::ReadyButUntested],
        recipes: Vec::new(),
        readiness_probes: [("qa-ready".into(), readiness)].into_iter().collect(),
        artifact_specs: [("qa-artifacts".into(), vec!["dist/index.html".into()])]
            .into_iter()
            .collect(),
        confidence: 80,
        evidence_refs: Vec::new(),
        user_overrides: vec![build.clone(), start.clone()],
        revision: 0,
    };
    runtime.save_manifest(&manifest, 0).unwrap();
    manifest.revision = 1;
    for (operation, recipe) in [
        (RuntimeOperation::Build, &build),
        (RuntimeOperation::Start, &start),
    ] {
        let binding = ProjectRuntimeStore::user_recipe_approval_binding(
            &manifest, &target, operation, recipe,
        )
        .unwrap();
        crate::persistence::runtime_target_approvals::save(&binding).unwrap();
    }

    super::verify_provenance_for_request(&runtime, &manifest, &target).unwrap();
    let revoked = ProjectRuntimeStore::user_recipe_approval_binding(
        &manifest,
        &target,
        RuntimeOperation::Start,
        &start,
    )
    .unwrap();
    crate::persistence::runtime_target_approvals::revoke(&revoked).unwrap();
    assert!(super::verify_provenance_for_request(&runtime, &manifest, &target).is_err());
}

#[test]
fn criteria_are_taken_from_exact_task_acceptance_section() {
    let task = "# Task\n\n## Purpose\n\nDo not treat this as a criterion.\n\n## Acceptance criteria\n\n- First behavior works.\n- Second behavior persists.\n\n## Verification\n\n- Run checks.\n";
    assert_eq!(
        acceptance_criteria(task).unwrap(),
        ["First behavior works.", "Second behavior persists."]
    );
}

#[test]
fn criteria_parser_fails_closed_on_unstructured_text() {
    assert!(acceptance_criteria("## Acceptance criteria\n\nUnstructured text\n").is_err());
    assert!(acceptance_criteria("## Purpose\n\n- Not acceptance criteria\n").is_err());
}

fn build_receipt(
    state: RuntimeReceiptState,
    target_id: &str,
    source_sha: &str,
    observed_at_unix_ms: u64,
) -> RuntimeReceiptV1 {
    RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        operation: RuntimeOperation::Build,
        task_id: "planning-item".into(),
        target_id: target_id.into(),
        source_sha: source_sha.into(),
        recipe_id: Some("web-build".into()),
        state,
        exit_code: (state == RuntimeReceiptState::Passed).then_some(0),
        artifact_hashes: Default::default(),
        log_reference: None,
        observed_at_unix_ms,
    }
}

#[test]
fn qa_build_lookup_uses_only_the_latest_pass_for_exact_target_and_sha() {
    let sha = "a".repeat(40);
    let receipts = [
        build_receipt(RuntimeReceiptState::Passed, "web", &"b".repeat(40), 9),
        build_receipt(RuntimeReceiptState::Passed, "api", &sha, 10),
        build_receipt(RuntimeReceiptState::Failed, "web", &sha, 11),
        build_receipt(RuntimeReceiptState::Running, "web", &sha, 12),
        build_receipt(RuntimeReceiptState::Passed, "web", &sha, 13),
    ];
    let matched = latest_successful_build_receipt(&receipts, "web", &sha, "web-build").unwrap();
    assert_eq!(matched.observed_at_unix_ms, 13);
}

#[test]
fn qa_start_lookup_requires_the_exact_task_target_head_and_approved_recipe() {
    let sha = "a".repeat(40);
    let mut passed = build_receipt(RuntimeReceiptState::Passed, "web", &sha, 13);
    passed.operation = RuntimeOperation::Start;
    passed.recipe_id = Some("web-start".into());
    let mut wrong_task = passed.clone();
    wrong_task.task_id = "other-task".into();
    let mut wrong_operation = passed.clone();
    wrong_operation.operation = RuntimeOperation::Test;
    let receipts = [wrong_task, wrong_operation, passed.clone()];

    let matched =
        latest_start_receipt(&receipts, "planning-item", "web", &sha, "web-start").unwrap();
    assert_eq!(matched.receipt_id, passed.receipt_id);

    let mut failed = passed.clone();
    failed.state = RuntimeReceiptState::Failed;
    failed.observed_at_unix_ms += 1;
    let receipts = [passed, failed];
    let latest =
        latest_start_receipt(&receipts, "planning-item", "web", &sha, "web-start").unwrap();
    assert_eq!(latest.state, RuntimeReceiptState::Failed);
}

#[test]
fn running_terminal_receipt_state_is_valid_and_not_a_pass() {
    let running = build_receipt(RuntimeReceiptState::Running, "web", &"a".repeat(40), 1);
    running.validate().unwrap();
    assert!(
        latest_successful_build_receipt(&[running], "web", &"a".repeat(40), "web-build").is_none()
    );
}

#[test]
fn qa_artifact_and_start_paths_are_rooted_at_the_workspace_clone() {
    assert_eq!(
        workspace_target_path("apps/web", "dist/index.html").unwrap(),
        "apps/web/dist/index.html"
    );
    assert_eq!(workspace_target_path("apps/web", ".").unwrap(), "apps/web");
    assert_eq!(
        workspace_target_path(".", "dist/index.html").unwrap(),
        "dist/index.html"
    );
    assert_eq!(
        workspace_target_path("./", "dist/index.html").unwrap(),
        "dist/index.html"
    );
}
