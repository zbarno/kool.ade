use super::*;

fn seeded_review_input(fixture: &SeededReviewEval) -> RoleInputV1 {
    let role = PromptRole::CodeReviewer;
    RoleInputV1 {
        role,
        workspace_uid: "seeded-review-eval".into(),
        feature_uid: "seeded-review-eval-feature".into(),
        task_uid: Some("seeded-review-eval-task".into()),
        original_goal: "Implement only the approved retry behavior.".into(),
        approved_spec_ref: Some("seeded-review-eval-spec".into()),
        approved_spec_hash: Some("b".repeat(64)),
        decision_refs: Vec::new(),
        evidence_manifest: vec![
            EvidenceItem {
                id: "approved-task-spec".into(),
                workspace_uid: "seeded-review-eval".into(),
                origin: EvidenceOrigin::Application,
                confidentiality: EvidenceConfidentiality::WorkspacePrivate,
                source: "approved task specification".into(),
                content: "Implement the required retry behavior only. Do not add unrelated metrics or abstractions without a requirement.".into(),
            },
            EvidenceItem {
                id: "diff-0".into(),
                workspace_uid: "seeded-review-eval".into(),
                origin: EvidenceOrigin::Application,
                confidentiality: EvidenceConfidentiality::WorkspacePrivate,
                source: "complete immutable git diff; chunks form the full diff".into(),
                content: fixture.diff.clone(),
            },
            EvidenceItem {
                id: "verified-test-report-0".into(),
                workspace_uid: "seeded-review-eval".into(),
                origin: EvidenceOrigin::Application,
                confidentiality: EvidenceConfidentiality::WorkspacePrivate,
                source: "application-validated verification report".into(),
                content: "Focused tests passed for the retry behavior.".into(),
            },
        ],
        execution_policy_ref: "code-review/read-only-immutable-diff/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: "seeded-review-eval-revision".into(),
            subject_hash: fixture.subject_hash.clone(),
            author_run_id: "00000000-0000-4000-8000-000000000001".into(),
            reviewer_run_id: "00000000-0000-4000-8000-000000000002".into(),
            author_session_id: Some("00000000-0000-4000-8000-000000000003".into()),
            reviewer_session_id: Some("00000000-0000-4000-8000-000000000004".into()),
            requirements_matrix: fixture.requirements.clone(),
            explicit_exclusions: Vec::new(),
        },
    }
}

#[derive(serde::Deserialize)]
struct SeededReviewEval {
    id: String,
    subject_hash: String,
    requirements: Vec<String>,
    diff: String,
    expected_categories: Vec<String>,
    expected_files: Vec<String>,
    expected_finding_paths: std::collections::HashMap<String, String>,
    output: serde_json::Value,
}

#[test]
fn seeded_review_eval_detects_overengineering_and_unrelated_changes() {
    let fixtures: Vec<SeededReviewEval> = serde_json::from_str(include_str!(
        "../../../../prompts/evals/v2/code_reviews.json"
    ))
    .unwrap();
    assert_eq!(fixtures.len(), 1);
    let fixture = &fixtures[0];
    assert_eq!(fixture.id, "seeded-overengineering-and-unrelated-change");
    assert!(fixture.diff.contains("retry_once"));
    assert!(fixture.diff.contains("count_widget_clicks"));
    let actual = fixture.output["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["kind"].as_str().unwrap().to_owned())
        .collect::<std::collections::HashSet<_>>();
    let expected = fixture
        .expected_categories
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(actual, expected, "{} finding oracle", fixture.id);
    assert_eq!(fixture.output["pass"], false);
    let examined_files = fixture.output["examined_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path.as_str().unwrap().to_owned())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        examined_files,
        fixture.expected_files.iter().cloned().collect()
    );
    for finding in fixture.output["findings"].as_array().unwrap() {
        let category = finding["kind"].as_str().unwrap();
        assert_eq!(
            finding["location_detail"]["file"],
            fixture.expected_finding_paths[category]
        );
    }
    assert!(
        fixture.output["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| {
                finding["scope"] == "blocking"
                    && finding["evidence"] == serde_json::json!(["diff-0"])
            })
    );

    let context = OutputValidationContext {
        expected_subject_hash: Some(&fixture.subject_hash),
        evidence_ids: ["diff-0".into(), "verified-test-report-0".into()].into(),
        required_requirements: fixture.requirements.clone(),
        ..Default::default()
    };
    assert!(
        validate_output_with_context(
            PromptRole::CodeReviewer,
            &fixture.output.to_string(),
            &context,
        )
        .is_ok()
    );
    let system = PromptRole::CodeReviewer.system();
    assert!(system.contains("overengineering"));
    assert!(system.contains("unrelated changes"));

    let rendered = render(&seeded_review_input(fixture)).unwrap();
    assert!(rendered.user.contains("retry_once"));
    assert!(rendered.user.contains("count_widget_clicks"));
    assert!(rendered.user.contains("diff-0"));
}

#[test]
#[ignore = "opt-in live reviewer eval; run with KOOLADE_RUN_LIVE_CODE_REVIEW_EVAL=1 and provider auth configured"]
fn live_reviewer_detects_seeded_overengineering_and_unrelated_changes() {
    assert_eq!(
        std::env::var("KOOLADE_RUN_LIVE_CODE_REVIEW_EVAL").as_deref(),
        Ok("1"),
        "set KOOLADE_RUN_LIVE_CODE_REVIEW_EVAL=1 to opt in"
    );
    use crate::harness::{AiHarness, ExecutionMode, PlanningRequest, ReviewExecutionIdentity};
    use std::sync::{Arc, atomic::AtomicBool, mpsc};
    use std::time::Duration;

    let fixtures: Vec<SeededReviewEval> = serde_json::from_str(include_str!(
        "../../../../prompts/evals/v2/code_reviews.json"
    ))
    .unwrap();
    let fixture = &fixtures[0];
    let input = seeded_review_input(fixture);
    let rendered = render(&input).unwrap();
    let identity = ReviewExecutionIdentity {
        execution_id: "00000000-0000-4000-8000-000000000002".into(),
        session_id: "00000000-0000-4000-8000-000000000004".into(),
        runtime_qa_screenshot_review: None,
    };
    let request = PlanningRequest {
        mode: ExecutionMode::ReadOnlyAnalysis,
        task_id: Some("seeded-review-eval-task".into()),
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        prompt_manifest: Some(rendered.manifest),
        reasoning_level: "high".into(),
        telemetry_phase: Some("independent_code_review".into()),
        repo_root: std::env::current_dir().unwrap(),
        runtime_config_source: None,
        prompt_body: rendered.user,
        system_instructions: rendered.system,
        timeout: Duration::from_secs(300),
        progress_tx: mpsc::channel().0,
        cancel: Arc::new(AtomicBool::new(false)),
        review_identity: Some(identity.clone()),
    };
    let harness = crate::harness::pi_harness::PiHarness;
    harness.check_available().expect("configured Pi reviewer");
    let response = harness
        .execute_review(&request, &identity)
        .expect("live CodeReviewer response");
    let context = OutputValidationContext {
        expected_subject_hash: Some(&fixture.subject_hash),
        evidence_ids: [
            "approved-task-spec".into(),
            "diff-0".into(),
            "verified-test-report-0".into(),
        ]
        .into(),
        required_requirements: fixture.requirements.clone(),
        ..Default::default()
    };
    let verdict = match validate_output_with_context(
        PromptRole::CodeReviewer,
        &response.final_text,
        &context,
    )
    .expect("live reviewer must satisfy the CodeReviewer schema")
    {
        RoleOutputV1::Review(verdict) => verdict,
        _ => panic!("live CodeReviewer returned an unexpected role output"),
    };
    assert!(!verdict.pass, "seeded defects must fail review");
    assert_eq!(
        verdict.outcome,
        Some(crate::harness::prompt_catalog::ReviewOutcomeV1::BlockingFindings),
        "seeded defects must produce blocking findings"
    );
    let actual = verdict
        .findings
        .iter()
        .map(|finding| {
            serde_json::to_value(&finding.kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect::<std::collections::HashSet<_>>();
    let examined_files = verdict
        .examined_files
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        examined_files,
        fixture.expected_files.iter().cloned().collect()
    );
    for expected in &fixture.expected_categories {
        assert!(
            actual.contains(expected),
            "live reviewer missed seeded finding `{expected}`; observed {actual:?}"
        );
        let expected_path = &fixture.expected_finding_paths[expected];
        assert!(
            verdict.findings.iter().any(|finding| {
                serde_json::to_value(&finding.kind)
                    .unwrap()
                    .as_str()
                    .is_some_and(|category| category == expected)
                    && finding
                        .location_detail
                        .as_ref()
                        .is_some_and(|location| location.file == *expected_path)
                    && finding.scope
                        == Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking)
            }),
            "live reviewer did not report blocking `{expected}` in `{expected_path}`"
        );
    }
}
