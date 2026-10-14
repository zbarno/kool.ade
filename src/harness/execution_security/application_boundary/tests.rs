use super::*;
use crate::harness::{ExecutionMode, ToolAccess};
use std::sync::{Arc, atomic::AtomicBool, mpsc};

#[test]
fn context_only_request_does_not_probe_or_require_a_repository_sandbox() {
    let (progress_tx, _progress_rx) = mpsc::channel();
    let request = PlanningRequest {
        prompt_manifest: None,
        mode: ExecutionMode::ReadOnlyAnalysis,
        task_id: None,
        reasoning_level: "off".into(),
        telemetry_phase: None,
        repo_root: PathBuf::new(),
        runtime_config_source: None,
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: None,
        authorization_verifier: None,
        review_identity: None,
    };

    let boundary = ApplicationBoundary::new(&request).unwrap();

    assert_eq!(boundary.access, ToolAccess::None);
    assert!(boundary.server_config.is_none());
    assert!(boundary.app_binary.is_none());
}

#[test]
fn implementation_without_an_application_selected_task_workspace_fails_closed() {
    let (progress_tx, _progress_rx) = mpsc::channel();
    let request = PlanningRequest {
        prompt_manifest: None,
        mode: ExecutionMode::Implementation,
        task_id: Some("task-1".into()),
        reasoning_level: "off".into(),
        telemetry_phase: None,
        repo_root: PathBuf::new(),
        runtime_config_source: None,
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: None,
        authorization_verifier: None,
        review_identity: None,
    };

    let error = match ApplicationBoundary::new(&request) {
        Ok(_) => panic!("implementation without an owned workspace must fail closed"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("application-selected task workspace")
    );
}

#[test]
fn requested_read_only_workspace_connection_failure_is_reported() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("DOCKER_CONTEXT", "untrusted-koolade-test-context");
    let (progress_tx, _progress_rx) = mpsc::channel();
    let request = PlanningRequest {
        prompt_manifest: None,
        mode: ExecutionMode::Planning,
        task_id: Some("task-1".into()),
        reasoning_level: "off".into(),
        telemetry_phase: None,
        repo_root: PathBuf::new(),
        runtime_config_source: None,
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: Some(crate::execution::contract::WorkspaceRef {
            project_id: "00000000-0000-4000-8000-000000000001".into(),
            task_id: "00000000-0000-4000-8000-000000000002".into(),
            source_sha: "0123456789012345678901234567890123456789".into(),
        }),
        authorization_verifier: None,
        review_identity: None,
    };

    let error = match ApplicationBoundary::new(&request) {
        Ok(_) => panic!("requested read-only workspace must not silently downgrade"),
        Err(error) => error,
    };

    assert!(
        error
            .to_string()
            .contains("requested read-only Docker planning session")
    );
    assert!(
        format!("{error:#}").contains("local Docker Engine"),
        "unexpected actionable failure: {error:#}"
    );
}

#[test]
fn reviewer_request_without_bound_prompt_and_session_fails_before_boundary_creation() {
    let (progress_tx, _progress_rx) = mpsc::channel();
    let request = PlanningRequest {
        prompt_manifest: None,
        mode: ExecutionMode::ReadOnlyAnalysis,
        task_id: None,
        reasoning_level: "off".into(),
        telemetry_phase: Some("independent_code_review".into()),
        repo_root: PathBuf::new(),
        runtime_config_source: None,
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: None,
        authorization_verifier: None,
        review_identity: Some(crate::harness::ReviewExecutionIdentity {
            execution_id: "00000000-0000-4000-8000-000000000001".into(),
            session_id: "00000000-0000-4000-8000-000000000002".into(),
        }),
    };

    let error = match ApplicationBoundary::new(&request) {
        Ok(_) => panic!("unbound reviewer request must fail before provider startup"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("CodeReviewer prompt"));
}
