use super::*;

fn request(mode: ExecutionMode) -> PlanningRequest {
    PlanningRequest {
        mode,
        task_id: Some("task-1".into()),
        execution_workspace: Some(crate::execution::contract::WorkspaceRef {
            project_id: "00000000-0000-4000-8000-000000000001".into(),
            task_id: "00000000-0000-4000-8000-000000000002".into(),
            source_sha: "a".repeat(40),
        }),
        destination_sha: None,
        authorization_verifier: None,
        prompt_manifest: None,
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: std::env::temp_dir(),
        runtime_config_source: None,
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(5),
        progress_tx: std::sync::mpsc::channel().0,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        review_identity: None,
    }
}

fn failure(error: AppError) -> HarnessExecutionFailure {
    let AppError::HarnessExecutionFailed { failure } = error else {
        panic!("provider error was not normalized")
    };
    *failure
}

#[test]
fn provider_auth_failure_is_typed_at_the_selected_role_and_workspace() {
    let request = request(ExecutionMode::Planning);
    let error = normalize_provider_error(
        "codex",
        HarnessRole::PlanningAnalyst,
        &request,
        AppError::HarnessFailed {
            reason: "authentication expired for https://user:token@example.invalid/path".into(),
            stderr_tail: String::new(),
        },
    );
    let failure = failure(error);
    assert_eq!(failure.provider_id, "codex");
    assert_eq!(failure.role, HarnessRole::PlanningAnalyst);
    assert_eq!(failure.mode, ExecutionMode::Planning);
    assert_eq!(
        failure.target.as_deref(),
        Some("00000000-0000-4000-8000-000000000002")
    );
    assert_eq!(failure.category, HarnessFailureCategory::Authentication);
    assert!(!failure.sanitized_message.contains("token"));
    assert!(!failure.retryable);
}

#[test]
fn missing_or_disappeared_provider_is_a_typed_unavailable_failure() {
    let request = request(ExecutionMode::Planning);
    for error in [
        AppError::HarnessNotFound {
            detail: "Codex CLI executable not found".into(),
        },
        AppError::Other("Cannot resolve Codex CLI executable: file disappeared".into()),
    ] {
        let failure = failure(normalize_provider_error(
            "codex",
            HarnessRole::PlanningAnalyst,
            &request,
            error,
        ));
        assert_eq!(
            failure.category,
            HarnessFailureCategory::ProviderUnavailable
        );
        assert!(!failure.retryable);
        assert!(
            failure
                .actionable_fix
                .unwrap()
                .contains("will not switch providers")
        );
    }
}

#[test]
fn host_code_mode_error_stays_separate_from_app_tool_bridge_readiness() {
    let request = request(ExecutionMode::Implementation);
    let error = normalize_provider_error(
        "codex",
        HarnessRole::Implementer,
        &request,
        AppError::HarnessFailed {
            reason: "Codex host code mode is disabled by Kool.ad/e policy".into(),
            stderr_tail: String::new(),
        },
    );
    let failure = failure(error);
    assert_eq!(
        failure.category,
        HarnessFailureCategory::HostCodeModeDisabled
    );
    assert!(
        failure
            .actionable_fix
            .unwrap()
            .contains("Docker tool session")
    );
    assert!(!failure.sanitized_message.contains("bridge unavailable"));
}

#[test]
fn structured_provider_error_code_is_preserved_without_free_text_values() {
    let request = request(ExecutionMode::Planning);
    let failure = failure(normalize_provider_error(
        "codex",
        HarnessRole::PlanningAnalyst,
        &request,
        AppError::HarnessFailed {
            reason: "provider request failed".into(),
            stderr_tail:
                r#"{"type":"error","error":{"code":"model_not_found","message":"token=secret"}}"#
                    .into(),
        },
    ));
    assert_eq!(failure.provider_code.as_deref(), Some("model_not_found"));
    assert!(!failure.sanitized_message.contains("secret"));
}

#[test]
fn timed_out_provider_failure_is_typed_and_not_automatically_retryable() {
    let request = request(ExecutionMode::Investigation);
    let failure = failure(normalize_provider_error(
        "claude",
        HarnessRole::PlanningAnalyst,
        &request,
        AppError::HarnessTimedOut { secs: 5 },
    ));
    assert_eq!(failure.category, HarnessFailureCategory::TimedOut);
    assert!(!failure.retryable);
}

#[test]
fn docker_target_failure_is_typed_only_when_execution_session_setup_failed() {
    let request = request(ExecutionMode::Implementation);
    let failure = failure(normalize_provider_error(
        "codex",
        HarnessRole::Implementer,
        &request,
        AppError::Other(
            "Cannot start Kool.ad/e sandbox: Docker implementation session unavailable: daemon is not reachable".into(),
        ),
    ));
    assert_eq!(
        failure.category,
        HarnessFailureCategory::ToolBridgeUnavailable
    );
    assert!(
        failure
            .sanitized_message
            .contains("daemon is not reachable")
    );
    assert!(
        failure
            .actionable_fix
            .unwrap()
            .contains("no host command fallback")
    );
}

#[test]
fn unsupported_application_execution_policy_is_not_reported_as_a_docker_outage() {
    let request = request(ExecutionMode::Implementation);
    let failure = failure(normalize_provider_error(
        "codex",
        HarnessRole::Implementer,
        &request,
        AppError::HarnessFailed {
            reason: "Codex repository access is unavailable because it has no configured application-owned Docker execution policy.".into(),
            stderr_tail: String::new(),
        },
    ));
    assert_eq!(
        failure.category,
        HarnessFailureCategory::UnsupportedProviderCapability
    );
    assert!(
        !failure
            .actionable_fix
            .unwrap()
            .contains("Docker execution readiness")
    );
}

#[test]
fn non_provider_errors_are_not_reclassified() {
    let request = request(ExecutionMode::Planning);
    let error = AppError::Other("planner state is invalid".into());
    assert_eq!(
        normalize_provider_error("codex", HarnessRole::Manager, &request, error.clone()),
        error
    );
}
