use super::*;
use crate::execution::contract::ResourceBudgetV1;
use crate::harness::{ExecutionMode, ToolAccess};
use std::sync::{Arc, atomic::AtomicBool, mpsc};

#[path = "tests/runtime_qa_empty_review.rs"]
mod runtime_qa_empty_review;

#[test]
fn resource_budget_overrides_are_task_scoped_and_restored() {
    let budget = ResourceBudgetV1 {
        cpu_millis: 1_000,
        memory_bytes: 512 * 1024 * 1024,
        process_count: 128,
        temporary_disk_bytes: 1024 * 1024 * 1024,
        output_bytes: 8 * 1024 * 1024,
    };
    let later_budget = ResourceBudgetV1 {
        cpu_millis: 500,
        ..budget.clone()
    };
    let overrides = override_test_resource_budgets([
        ("task-1".into(), budget.clone()),
        ("task-1".into(), later_budget.clone()),
    ]);
    assert_eq!(default_resource_budget(Some("task-1")), later_budget);
    assert_ne!(default_resource_budget(Some("task-2")), budget);
    drop(overrides);
    assert_eq!(
        default_resource_budget(Some("task-1")),
        ResourceBudgetV1::default()
    );
}

#[cfg(target_os = "linux")]
#[path = "tests/docker_provider_mcp.rs"]
mod docker_provider_mcp;

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
        destination_sha: None,
        authorization_verifier: None,
        review_identity: None,
    };

    let boundary = ApplicationBoundary::new(&request).unwrap();

    assert_eq!(boundary.access, ToolAccess::None);
    assert!(boundary.server_config.is_none());
    assert!(boundary.execution_session.is_none());
}

// Production boundary: ApplicationBoundary receives an ordinary Planning
// request without an application-selected workspace. This proves that context
// only routing grants no repository tools; a readiness unit test cannot prove
// the request boundary preserves that restriction. It needs no Docker or SSH,
// only a synthetic request and a temporary private directory, so setup is low.
#[test]
fn planning_without_a_workspace_receives_no_repository_tools() {
    let (progress_tx, _progress_rx) = mpsc::channel();
    let request = PlanningRequest {
        prompt_manifest: None,
        mode: ExecutionMode::Planning,
        task_id: None,
        reasoning_level: "off".into(),
        telemetry_phase: None,
        repo_root: PathBuf::new(),
        runtime_config_source: None,
        prompt_body: "synthetic context".into(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        review_identity: None,
    };

    let boundary = ApplicationBoundary::new(&request).unwrap();

    assert_eq!(boundary.access, ToolAccess::None);
    assert!(boundary.server_config.is_none());
    assert!(boundary.execution_session.is_none());
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
        destination_sha: None,
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
        destination_sha: None,
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
        destination_sha: None,
        authorization_verifier: None,
        review_identity: Some(crate::harness::ReviewExecutionIdentity {
            execution_id: "00000000-0000-4000-8000-000000000001".into(),
            session_id: "00000000-0000-4000-8000-000000000002".into(),
            runtime_qa_screenshot_review: None,
        }),
    };

    let error = match ApplicationBoundary::new(&request) {
        Ok(_) => panic!("unbound reviewer request must fail before provider startup"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("reviewer prompt"));
}
