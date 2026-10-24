use super::*;
use crate::harness::{
    ExecutionMode, ReviewExecutionIdentity, RuntimeQaScreenshotReview,
    prompt_catalog::PromptManifest,
};
use std::time::Duration;

/// Production boundary: a validated independent headless review becomes an
/// app-owned MCP policy with an empty screenshot allowlist. Prompt-only tests
/// cannot prove boundary construction accepts the no-image case. No Docker,
/// SSH, browser, or network is required; setup uses a private temp directory.
#[test]
fn headless_runtime_qa_boundary_accepts_empty_screenshot_set() {
    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let execution_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let (progress_tx, _progress_rx) = mpsc::channel();
    let request = PlanningRequest {
        prompt_manifest: Some(PromptManifest {
            role_version: "runtime_qa/v3".into(),
            system_template_sha256: "a".repeat(64),
            policy_template_sha256: "b".repeat(64),
            user_template_sha256: "c".repeat(64),
            output_schema_version: "ReviewVerdictV1".into(),
            privilege_profile: "read_only; isolated_fresh_review_context".into(),
        }),
        mode: ExecutionMode::RuntimeQaVisualReview,
        task_id: Some(task_id.clone()),
        reasoning_level: "high".into(),
        telemetry_phase: Some("runtime_qa_review".into()),
        repo_root: PathBuf::new(),
        runtime_config_source: None,
        prompt_body: format!("review execution {execution_id} session {session_id}"),
        system_instructions: "Review frozen headless QA evidence.".into(),
        timeout: Duration::from_secs(5),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        review_identity: Some(ReviewExecutionIdentity {
            execution_id,
            session_id,
            runtime_qa_screenshot_review: Some(Arc::new(RuntimeQaScreenshotReview {
                workspace_id: project_id,
                source_sha: "d".repeat(40),
                screenshots: Vec::new(),
                inspected_screenshot_ids: Arc::default(),
            })),
        }),
    };

    let boundary = ApplicationBoundary::new(&request).unwrap();
    assert_eq!(boundary.access(), ToolAccess::RuntimeQaScreenshots);
    let config_path = boundary.server_config.as_ref().unwrap();
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(config_path).unwrap()).unwrap();
    assert_eq!(
        config["screenshot_access"]["sha256_by_id"],
        serde_json::json!({})
    );
}
