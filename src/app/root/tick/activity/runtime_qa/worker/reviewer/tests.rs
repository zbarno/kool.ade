use super::*;

fn qa_request() -> QaRunRequestV1 {
    QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: "00000000-0000-4000-8000-000000000001".into(),
        parent_task_uid: "00000000-0000-4000-8000-000000000002".into(),
        target_id: Some("fixture-target".into()),
        planning_item_uid: None,
        feature_uid: "00000000-0000-4000-8000-000000000003".into(),
        code_head_sha: "a".repeat(40),
        build_artifact_sha256: "b".repeat(64),
        complete_diff_hash: "c".repeat(64),
        review_subject_sha256: "d".repeat(64),
        approved_spec_hash: "e".repeat(64),
        requirement_ids: vec!["REQ-1".into()],
        acceptance_criteria: vec!["Observed behavior matches requirement.".into()],
        relevant_platforms: vec![crate::runtime_qa::QaPlatform::Web],
        startup_recipe_ref: "recipe-1".into(),
        startup_recipe_sha256: "f".repeat(64),
        execution_session_id: "00000000-0000-4000-8000-000000000004".into(),
        driver_profile_version: "browser/v1".into(),
        test_data_fixture_ref: None,
        dependency_cache_refs: Vec::new(),
        bounded_limits: crate::runtime_qa::QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 1,
            max_total_actions: 1,
            timeout_seconds: 30,
            max_evidence_bytes: 1024,
            max_screenshots: 1,
        },
        author_run_id: "00000000-0000-4000-8000-000000000005".into(),
        author_session_id: "00000000-0000-4000-8000-000000000006".into(),
    }
}

#[test]
fn reviewer_evidence_uses_same_workspace_identity_as_role_input() {
    let request = qa_request();
    let workspace_uid = "00000000-0000-4000-8000-000000000007";
    let input = super::role_input(
        &request,
        "00000000-0000-4000-8000-000000000008",
        "00000000-0000-4000-8000-000000000009",
        workspace_uid,
        "frozen QA evidence".into(),
        "a".repeat(64),
        vec![EvidenceItem {
            id: "runtime-qa-evidence".into(),
            workspace_uid: workspace_uid.into(),
            origin: EvidenceOrigin::Tool,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: "runtime-qa://run".into(),
            content: "frozen QA evidence".into(),
        }],
    );
    let rendered = render(&input).expect("QA review context should satisfy the catalog contract");
    assert_eq!(rendered.manifest.role_version, "runtime_qa/v3");
    assert!(
        rendered
            .system
            .contains("You do not launch or control browser")
    );
    assert!(rendered.user.contains("do not run drivers"));
    assert!(!rendered.system.contains("Exercise the running application"));
    assert!(rendered.user.contains(workspace_uid));
}

#[test]
fn qa_request_validation_accepts_sha1_git_oid_with_sha256_review_subject() {
    let request = qa_request();
    request
        .validate()
        .expect("a 40-character Git OID and 64-character review hash are valid");

    let mut invalid_commit_oid = request.clone();
    invalid_commit_oid.code_head_sha = "g".repeat(40);
    assert!(invalid_commit_oid.validate().is_err());

    let mut invalid_review_subject = request.clone();
    invalid_review_subject.review_subject_sha256 = "d".repeat(40);
    assert!(invalid_review_subject.validate().is_err());
}

#[test]
fn headless_reviewer_context_accepts_action_evidence_references() {
    let actions = [crate::runtime_qa::QaActionEvidenceV1 {
        index: 0,
        scenario_id: "scenario-1".into(),
        command: "api_request".into(),
        args_redacted: std::collections::BTreeMap::new(),
        pre_state: None,
        post_state: Some("HTTP 200".into()),
        evidence_ids: vec!["api-response-1".into()],
        occurred_at_unix_ms: 1,
    }];
    let hash = "a".repeat(64);
    let context = crate::harness::prompt_catalog::OutputValidationContext {
        expected_subject_hash: Some(&hash),
        evidence_ids: super::reviewer_evidence_ids(&[], &actions)
            .into_iter()
            .collect(),
        required_requirements: vec!["REQ-1".into()],
        ..Default::default()
    };
    let output = serde_json::json!({
        "subject_hash": hash,
        "examined_files": [],
        "tested_evidence_ids": [],
        "unexamined_areas": [],
        "pass": true,
        "outcome": "pass",
        "coverage": [{
            "requirement": "REQ-1",
            "status": "pass",
            "evidence": ["0", "api-response-1"]
        }],
        "findings": [],
        "untested": []
    })
    .to_string();

    crate::harness::prompt_catalog::validate_output_with_context(
        crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV3,
        &output,
        &context,
    )
    .expect("reviewer citations may reference observed headless action evidence");
}
