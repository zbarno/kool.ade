use super::*;
use crate::harness::prompt_catalog::{
    CoverageStatus, CoverageV1, ReviewOutcomeV1, ReviewVerdictV1,
};
use crate::runtime_qa::{QaDriverActionV1, QaHttpMethodV1, QaScenarioStepV1};
use serde_json::json;

struct ParsedServiceDriver;

impl QaScenarioDriver for ParsedServiceDriver {
    fn platform(&self) -> QaPlatform {
        QaPlatform::HeadlessService
    }

    fn run_scenario(
        &mut self,
        request: &QaRunRequestV1,
        scenario: &QaScenarioV1,
        authorization: &QaTargetAuthorization<'_>,
    ) -> Result<QaDriverOutputV1, String> {
        if !matches!(authorization, QaTargetAuthorization::HeadlessService(_)) {
            return Err("headless service requires its app-owned lease".into());
        }
        let evidence_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let report = json!({
            "schema_version": 1,
            "qa_run_id": request.qa_run_id,
            "scenario_id": scenario.id,
            "task_uid": request.parent_task_uid,
            "code_sha": request.code_head_sha,
            "build_sha256": request.build_artifact_sha256,
            "approved_spec_hash": request.approved_spec_hash,
            "driver_profile_version": request.driver_profile_version,
            "outcome": "completed",
            "origin": "http://qa-preview:8080",
            "viewport": {"width": 1280, "height": 720},
            "driver_version": "node-pinned",
            "actions": [{
                "index": 0,
                "action": "api_request",
                "args_redacted": {"method": "GET", "path": "/health"},
                "pre_state": {"url": "http://qa-preview:8080", "title": "request pending"},
                "post_state": {"url": "http://qa-preview:8080/health", "title": "status=200"},
                "evidence_ids": [evidence_id],
                "response_status": 200,
                "response_sha256": "a".repeat(64),
                "body_matches": true,
                "status": "observed"
            }],
            "screenshots": [],
            "console_errors": [],
            "failed_requests": [],
            "policy_violation": null
        });
        let bytes = serde_json::to_vec(&report).map_err(|error| error.to_string())?;
        crate::runtime_qa::parse_browser_output(&bytes, request, scenario)
    }
}

// Production boundary: API-driver evidence passes through parsing, report
// assembly, result validation, durable storage, and gate receipt creation.
// A validator unit test cannot prove this screenshot-free path remains wired
// through orchestration. Uses a synthetic service report and in-process
// reviewer/store; no Docker, SSH, network, or credentials, with low setup cost.
#[test]
fn headless_service_pass_uses_action_evidence_without_screenshots() {
    let (mut request, mut scenarios, mut lease) = support::fixture();
    request.relevant_platforms = vec![QaPlatform::HeadlessService];
    request.driver_profile_version = "headless-service/v1".into();
    request.startup_recipe_ref = "recipe:service".into();
    request.acceptance_criteria = vec!["Health request returns HTTP 200".into()];
    lease.startup_recipe_ref = request.startup_recipe_ref.clone();

    let scenario = &mut scenarios[0];
    scenario.steps = vec![QaScenarioStepV1 {
        driver_action: QaDriverActionV1::ApiRequest {
            method: QaHttpMethodV1::Get,
            path: "/health".into(),
            json_body: None,
            expected_status: 200,
            expected_body_contains: None,
        },
        expected_state: Some("health request returns HTTP 200".into()),
        screenshot_checkpoint: false,
    }];
    scenario.expected_results = vec!["Health request returns HTTP 200".into()];
    scenario.visual_expectations.clear();

    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = support::reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = ParsedServiceDriver;
    let mut reviewer = |evidence: &QaEvidenceManifestV1,
                        candidate: &QaResultV1,
                        _screenshots: &[QaScreenshotArtifactV1]| {
        Ok(ReviewVerdictV1 {
            subject_hash: crate::runtime_qa::qa_review_subject_sha256(
                &request, candidate, evidence,
            )?,
            examined_files: Vec::new(),
            tested_evidence_ids: Vec::new(),
            unexamined_areas: Vec::new(),
            pass: true,
            outcome: Some(ReviewOutcomeV1::Pass),
            coverage: vec![CoverageV1 {
                requirement: request.requirement_ids[0].clone(),
                status: CoverageStatus::Pass,
                evidence: vec![
                    evidence.actions[0].index.to_string(),
                    evidence.actions[0].evidence_ids[0].clone(),
                ],
            }],
            findings: Vec::new(),
            untested: Vec::new(),
        })
    };

    let (result, receipt) = RuntimeQaOrchestrator::new(&store)
        .execute_reviewed(
            QaExecutionContext {
                request: &request,
                scenarios: &scenarios,
                authorization: QaTargetAuthorization::HeadlessService(&lease),
                driver: &mut driver,
                reviewer_execution_id: &reviewer_run,
                reviewer_session_id: &reviewer_session,
                prompt,
            },
            &mut reviewer,
        )
        .unwrap();

    assert_eq!(result.outcome, QaOutcome::Pass);
    assert!(receipt.is_some());
    let persisted = store.load_run_with_screenshots(&request.qa_run_id).unwrap();
    assert_eq!(persisted.evidence.platform, QaPlatform::HeadlessService);
    assert!(persisted.evidence.screenshots.is_empty());
    assert!(!persisted.evidence.actions[0].evidence_ids.is_empty());
    assert_eq!(persisted.result, result);
    assert_eq!(
        store.load_gate_receipt(&request.qa_run_id).unwrap(),
        receipt.unwrap()
    );

    let mut mixed_platform_request = request.clone();
    mixed_platform_request
        .relevant_platforms
        .push(QaPlatform::Web);
    assert!(
        result
            .validate(&mixed_platform_request, &scenarios, &persisted.evidence)
            .is_err()
    );
}
