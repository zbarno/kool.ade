use super::*;

#[test]
fn code_reviewer_receives_the_complete_subject_in_read_only_mode() {
    let state: Implementation = serde_json::from_value(serde_json::json!({
        "ticket": "planning/items/task.md",
        "task_uid": "00000000-0000-4000-8000-000000000002",
        "ticket_text": "Feature: 00000000-0000-4000-8000-000000000001\n- [ ] AC-1",
        "approved_specification": "approved requirement",
        "branch": "koolade/task",
        "base": "main",
        "base_commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "task_repository": "/tmp/reviewer-task-clone",
        "status": "verifying",
        "detail": "",
        "pr_url": null,
        "verified_head": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    }))
    .unwrap();
    let author = ReviewExecutionIdentity {
        execution_id: "00000000-0000-4000-8000-000000000003".into(),
        session_id: "00000000-0000-4000-8000-000000000004".into(),
        runtime_qa_screenshot_review: None,
    };
    let reviewer = ReviewExecutionIdentity {
        execution_id: "00000000-0000-4000-8000-000000000005".into(),
        session_id: "00000000-0000-4000-8000-000000000006".into(),
        runtime_qa_screenshot_review: None,
    };
    let diff = "diff --git a/src/lib.rs b/src/lib.rs\n+fn behavior() {}\n";
    let requirements = ["AC-1".into()];
    let (request, manifest, evidence_ids) = build_request(CodeReviewRequestInput {
        state: &state,
        feature_uid: "00000000-0000-4000-8000-000000000001",
        subject_hash: &"c".repeat(64),
        approved_plan_hash: &"d".repeat(64),
        diff,
        spec: "approved requirement",
        report: "{\"tests\":[\"cargo test\"]}",
        requirements: &requirements,
        subject_context: "{\"linked_subtask_ids\":[],\"runtime_qa_receipt_ids\":[]}",
        author: &author,
        reviewer: &reviewer,
        arbitration: None,
    })
    .unwrap();
    assert_eq!(request.mode, ExecutionMode::ReadOnlyAnalysis);
    assert!(request.execution_workspace.is_none());
    assert_eq!(request.review_identity.as_ref(), Some(&reviewer));
    assert_eq!(manifest.role_version, "code_review/v2");
    assert!(request.prompt_body.contains("diff --git a/src/lib.rs"));
    assert!(request.prompt_body.contains("approved requirement"));
    assert!(request.prompt_body.contains("review-subject-v1"));
    assert!(evidence_ids.contains("verified-test-report-0"));
    assert_eq!(request.task_id.as_deref(), state.task_uid.as_deref());
}
