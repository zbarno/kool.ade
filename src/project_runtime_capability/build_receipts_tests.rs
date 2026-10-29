use super::*;

fn target() -> AppTargetV1 {
    AppTargetV1 {
        target_id: "web-target".into(),
        owning_task_id: "task-1".into(),
        repository_id: "repo-1".into(),
        app_surface: crate::project_runtime::AppSurface::Web,
        project_relative_root: ".".into(),
        build_recipe_ref: Some("build-v1".into()),
        test_recipe_refs: vec!["test-v1".into()],
        start_recipe_ref: Some("start-v1".into()),
        readiness_probe_ref: Some("ready-v1".into()),
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: vec!["browser".into()],
        port_needs: Vec::new(),
        environment_policy_ref: "default-deny".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    }
}

fn build_receipt(
    task_id: &str,
    target_id: &str,
    source_sha: &str,
    recipe_id: &str,
    state: RuntimeReceiptState,
    observed_at_unix_ms: u64,
) -> RuntimeReceiptV1 {
    RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        operation: RuntimeOperation::Build,
        task_id: task_id.into(),
        target_id: target_id.into(),
        source_sha: source_sha.into(),
        recipe_id: Some(recipe_id.into()),
        state,
        exit_code: match state {
            RuntimeReceiptState::Passed => Some(0),
            RuntimeReceiptState::Failed => Some(1),
            RuntimeReceiptState::Running | RuntimeReceiptState::NotApplicable => None,
        },
        artifact_hashes: Default::default(),
        prepared_dependency_ids: Vec::new(),
        log_reference: None,
        observed_at_unix_ms,
    }
}

#[test]
fn latest_exact_source_build_failure_is_projected_without_stale_source_override() {
    let target = target();
    let current_sha = "b".repeat(40);
    let older_sha = "a".repeat(40);
    let current_pass = build_receipt(
        "task-1",
        "web-target",
        &current_sha,
        "build-v1",
        RuntimeReceiptState::Passed,
        10,
    );
    let later_old_source_failure = build_receipt(
        "task-1",
        "web-target",
        &older_sha,
        "build-v1",
        RuntimeReceiptState::Failed,
        20,
    );
    assert_eq!(
        observed_build_capability_state(
            &target,
            "task-1",
            &current_sha,
            &[current_pass.clone(), later_old_source_failure],
        ),
        None,
        "a later timestamp from an older source revision cannot replace current-source state"
    );

    let current_failure = build_receipt(
        "task-1",
        "web-target",
        &current_sha,
        "build-v1",
        RuntimeReceiptState::Failed,
        30,
    );
    assert_eq!(
        observed_build_capability_state(
            &target,
            "task-1",
            &current_sha,
            &[current_pass, current_failure],
        ),
        Some(TargetCapabilityState::BuildFailed),
        "the latest failure for the selected source must be shown"
    );
}
