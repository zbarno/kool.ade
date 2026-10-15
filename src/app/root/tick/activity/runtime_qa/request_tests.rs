use super::{
    latest_successful_build_receipt, task_spec::acceptance_criteria, workspace_target_path,
};
use crate::project_runtime::{RuntimeOperation, RuntimeReceiptState, RuntimeReceiptV1};

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
}
