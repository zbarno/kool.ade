use super::super::ApplicationBoundary;
use crate::harness::{ExecutionMode, PlanningRequest};
use std::sync::{Arc, atomic::AtomicBool, mpsc};

#[test]
fn project_guidance_mode_filter_covers_every_execution_mode() {
    for mode in ExecutionMode::ALL {
        let expected = matches!(
            mode,
            ExecutionMode::Planning
                | ExecutionMode::TaskGeneration
                | ExecutionMode::Investigation
                | ExecutionMode::Implementation
        );
        assert_eq!(
            super::super::construction::mode_uses_project_guidance(mode),
            expected,
            "unexpected guidance policy for {mode:?}"
        );
    }
}

// Production boundary: each ordinary planning launch reloads the per-project
// receipt before building provider context. The direct envelope unit test
// cannot prove persistence-to-boundary wiring or that a newly disabled receipt
// is omitted on the next launch. Setup uses a temporary Git repo and private
// local state; it needs no Docker, provider CLI, network, or credentials.
#[test]
fn planning_boundary_reloads_only_enabled_project_guidance() {
    let root = tempfile::tempdir().unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", root.path().join("state"));
    let repo = root.path().join("code");
    std::fs::create_dir(&repo).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );
    crate::persistence::project_guidance::save_for_root(
        &repo,
        "Use the current project naming conventions.",
        true,
    )
    .unwrap();
    let (progress_tx, _progress_rx) = mpsc::channel();
    let request = PlanningRequest {
        investigation_authorization: None,
        prompt_manifest: None,
        mode: ExecutionMode::Planning,
        task_id: None,
        reasoning_level: "off".into(),
        telemetry_phase: None,
        repo_root: repo,
        runtime_config_source: None,
        prompt_body: "Inspect the project.".into(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        review_identity: None,
        computer_use: None,
    };

    let first = ApplicationBoundary::new(&request).unwrap();
    let prompt_body = first.prompt_body(&request.prompt_body);
    assert!(prompt_body.contains("Use the current project naming conventions."));
    let mut provider_request = request.clone();
    provider_request.prompt_body = prompt_body;
    let (_, codex_user) = crate::harness::codex_harness::command(
        std::path::Path::new("codex"),
        &provider_request,
        None,
        std::path::Path::new("/private/run"),
    );
    assert!(codex_user.contains("Use the current project naming conventions."));
    let claude_user: serde_json::Value = serde_json::from_str(
        crate::harness::claude_harness::prompt_input(&provider_request.prompt_body).trim(),
    )
    .unwrap();
    assert!(
        claude_user
            .to_string()
            .contains("Use the current project naming conventions.")
    );
    let antigravity: serde_json::Value = serde_json::from_str(
        &crate::harness::antigravity_harness::prompt_input_with_policy(
            &provider_request,
            "boundary",
        ),
    )
    .unwrap();
    assert!(
        antigravity
            .to_string()
            .contains("Use the current project naming conventions.")
    );
    assert!(
        crate::harness::opencode_harness::prompt_input(&provider_request, "boundary")
            .contains("Use the current project naming conventions.")
    );
    assert!(
        crate::harness::copilot_harness::prompt_input(&provider_request, "boundary")
            .contains("Use the current project naming conventions.")
    );
    assert!(
        crate::harness::pi_harness::user_input(&provider_request)
            .contains("Use the current project naming conventions.")
    );
    let mut reviewer_request = request.clone();
    reviewer_request.mode = ExecutionMode::ReadOnlyAnalysis;
    let reviewer = ApplicationBoundary::new(&reviewer_request).unwrap();
    assert_eq!(
        reviewer.prompt_body(&reviewer_request.prompt_body),
        reviewer_request.prompt_body
    );
    crate::persistence::project_guidance::save_for_root(
        &request.repo_root,
        "Use the current project naming conventions.",
        false,
    )
    .unwrap();
    let second = ApplicationBoundary::new(&request).unwrap();
    assert_eq!(
        second.prompt_body(&request.prompt_body),
        request.prompt_body
    );

    crate::persistence::project_guidance::save_for_root(
        &request.repo_root,
        "Approved text before tampering.",
        true,
    )
    .unwrap();
    let project_id =
        crate::persistence::dependency_authorization::project_id(&request.repo_root).unwrap();
    let receipt_path =
        crate::persistence::project_dir(&project_id).join("approved-agent-guidance.json");
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&receipt_path).unwrap()).unwrap();
    receipt["content"] = "changed without approval".into();
    std::fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    let mut implementation_request = request.clone();
    implementation_request.mode = ExecutionMode::Implementation;
    let error = match ApplicationBoundary::new(&implementation_request) {
        Ok(_) => panic!("tampered guidance must block before implementation setup"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("approval does not match"));
}
