use super::{
    approved_candidate, can_queue_prerequisite_recovery, latest_receipt, next_operation,
    receipt_allows_next, supports_recipe_surface,
};
use crate::{
    execution::contract::WorkspaceRef,
    project_runtime::{
        AppSurface, AppTargetV1, ExecutionRecipeV1, NetworkRequirement,
        ProjectCapabilityManifestV1, RecipeSource, RuntimeOperation, RuntimeReceiptState,
        RuntimeReceiptV1, TargetCapabilityState,
    },
};

#[test]
fn generic_recipe_queue_accepts_headless_targets_but_rejects_unsupported_surfaces() {
    assert!(supports_recipe_surface(AppSurface::HeadlessService));
    assert!(supports_recipe_surface(AppSurface::Web));
    assert!(supports_recipe_surface(AppSurface::LinuxDesktop));
    assert!(!supports_recipe_surface(AppSurface::UnsupportedTarget));
}

#[test]
fn brokerable_capability_gaps_do_not_block_recipe_prerequisite_recovery() {
    for state in [
        TargetCapabilityState::Ready,
        TargetCapabilityState::ReadyButUntested,
        TargetCapabilityState::MissingToolchain,
        TargetCapabilityState::MissingDependencies,
        TargetCapabilityState::DriverUnavailable,
    ] {
        assert!(can_queue_prerequisite_recovery(&state), "{state:?}");
    }
    for state in [
        TargetCapabilityState::UnsupportedPlatform,
        TargetCapabilityState::BuildFailed,
        TargetCapabilityState::StartFailed,
    ] {
        assert!(!can_queue_prerequisite_recovery(&state), "{state:?}");
    }
}

fn receipt(state: RuntimeReceiptState, observed_at_unix_ms: u64) -> RuntimeReceiptV1 {
    operation_receipt(
        RuntimeOperation::Build,
        "build-v1",
        state,
        observed_at_unix_ms,
    )
}

fn operation_receipt(
    operation: RuntimeOperation,
    recipe_id: &str,
    state: RuntimeReceiptState,
    observed_at_unix_ms: u64,
) -> RuntimeReceiptV1 {
    RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        operation,
        task_id: "task-1".into(),
        target_id: "web".into(),
        source_sha: "a".repeat(40),
        recipe_id: Some(recipe_id.into()),
        state,
        exit_code: (state == RuntimeReceiptState::Passed).then_some(0),
        artifact_hashes: Default::default(),
        prepared_dependency_ids: Vec::new(),
        log_reference: None,
        observed_at_unix_ms,
    }
}

#[test]
fn runtime_recipe_queue_requires_build_test_then_start_readiness_receipts() {
    let mut target = AppTargetV1 {
        target_id: "web".into(),
        owning_task_id: "task-1".into(),
        repository_id: "repo".into(),
        app_surface: AppSurface::Web,
        project_relative_root: "apps/web".into(),
        build_recipe_ref: Some("build-v1".into()),
        test_recipe_refs: vec!["test-v1".into()],
        start_recipe_ref: Some("start-v1".into()),
        readiness_probe_ref: Some("ready-v1".into()),
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
    let source_sha = "a".repeat(40);
    let build = operation_receipt(
        RuntimeOperation::Build,
        "build-v1",
        RuntimeReceiptState::Passed,
        10,
    );
    let test = operation_receipt(
        RuntimeOperation::Test,
        "test-v1",
        RuntimeReceiptState::Passed,
        20,
    );
    let start = operation_receipt(
        RuntimeOperation::Start,
        "start-v1",
        RuntimeReceiptState::Passed,
        30,
    );

    assert_eq!(
        next_operation(&target, &[], &source_sha).unwrap(),
        Some((RuntimeOperation::Build, vec!["build-v1".into()]))
    );
    assert_eq!(
        next_operation(&target, std::slice::from_ref(&build), &source_sha).unwrap(),
        Some((RuntimeOperation::Test, vec!["test-v1".into()]))
    );
    assert_eq!(
        next_operation(&target, &[build.clone(), test.clone()], &source_sha).unwrap(),
        Some((RuntimeOperation::Start, vec!["start-v1".into()]))
    );
    assert_eq!(
        next_operation(
            &target,
            &[build.clone(), test.clone(), start.clone()],
            &source_sha
        )
        .unwrap(),
        None
    );

    let failed_start = operation_receipt(
        RuntimeOperation::Start,
        "start-v1",
        RuntimeReceiptState::Failed,
        40,
    );
    assert!(
        next_operation(
            &target,
            &[build.clone(), test.clone(), failed_start],
            &source_sha
        )
        .is_err()
    );
    target.readiness_probe_ref = None;
    assert!(next_operation(&target, &[build, test], &source_sha).is_err());
}

#[test]
fn headless_targets_run_approved_build_test_and_start_recipes_without_visual_claims() {
    let mut target = AppTargetV1 {
        target_id: "service".into(),
        owning_task_id: "task-1".into(),
        repository_id: "repo".into(),
        app_surface: AppSurface::HeadlessService,
        project_relative_root: "services/api".into(),
        build_recipe_ref: Some("build-v1".into()),
        test_recipe_refs: vec!["test-v1".into()],
        start_recipe_ref: Some("start-v1".into()),
        readiness_probe_ref: Some("ready-v1".into()),
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
    let source_sha = "a".repeat(40);
    let build = RuntimeReceiptV1 {
        target_id: "service".into(),
        ..operation_receipt(
            RuntimeOperation::Build,
            "build-v1",
            RuntimeReceiptState::Passed,
            10,
        )
    };
    let test = RuntimeReceiptV1 {
        operation: RuntimeOperation::Test,
        target_id: "service".into(),
        ..operation_receipt(
            RuntimeOperation::Test,
            "test-v1",
            RuntimeReceiptState::Passed,
            20,
        )
    };

    assert!(supports_recipe_surface(target.app_surface));
    assert_eq!(
        next_operation(&target, &[], &source_sha).unwrap(),
        Some((RuntimeOperation::Build, vec!["build-v1".into()]))
    );
    assert_eq!(
        next_operation(&target, std::slice::from_ref(&build), &source_sha).unwrap(),
        Some((RuntimeOperation::Test, vec!["test-v1".into()]))
    );
    assert_eq!(
        next_operation(&target, &[build.clone(), test.clone()], &source_sha).unwrap(),
        Some((RuntimeOperation::Start, vec!["start-v1".into()]))
    );
    let start = RuntimeReceiptV1 {
        operation: RuntimeOperation::Start,
        target_id: "service".into(),
        ..operation_receipt(
            RuntimeOperation::Start,
            "start-v1",
            RuntimeReceiptState::Passed,
            30,
        )
    };
    assert_eq!(
        next_operation(&target, &[build, test, start], &source_sha).unwrap(),
        None
    );

    target.app_surface = AppSurface::UnsupportedTarget;
    assert!(!supports_recipe_surface(target.app_surface));
}

#[test]
fn restart_does_not_replay_an_attempt_claimed_before_execution() {
    let old_success = receipt(RuntimeReceiptState::Passed, 10);
    let interrupted_attempt = receipt(RuntimeReceiptState::Running, 20);
    let receipts = [old_success, interrupted_attempt];
    let latest = latest_receipt(
        &receipts,
        RuntimeOperation::Build,
        "web",
        &"a".repeat(40),
        "build-v1",
    )
    .unwrap();
    assert_eq!(latest.state, RuntimeReceiptState::Running);
    assert!(receipt_allows_next(latest, "build").is_err());
}

#[test]
fn failed_attempt_requires_a_fresh_explicit_approval() {
    let failed = receipt(RuntimeReceiptState::Failed, 30);
    assert!(receipt_allows_next(&failed, "test").is_err());
    assert!(receipt_allows_next(&receipt(RuntimeReceiptState::Passed, 31), "test").is_ok());
}

#[path = "recipe_queue_tests/approval.rs"]
mod approval;
