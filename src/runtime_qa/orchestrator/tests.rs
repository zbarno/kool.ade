#[path = "tests/reviewer.rs"]
mod reviewer;

use super::*;
use crate::harness::prompt_catalog::{
    CoverageStatus, CoverageV1, EvidenceItem, ReviewVerdictV1, RoleInputV1, RolePayloadV1, render,
};
use sha2::Digest as _;
use std::io::Cursor;

struct FakeDriver {
    calls: usize,
    assertion_status: QaAssertionStatus,
}

impl QaScenarioDriver for FakeDriver {
    fn platform(&self) -> QaPlatform {
        QaPlatform::Web
    }

    fn run_scenario(
        &mut self,
        request: &QaRunRequestV1,
        scenario: &QaScenarioV1,
        _authorization: &QaTargetAuthorization<'_>,
    ) -> Result<QaDriverOutputV1, String> {
        self.calls += 1;
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(16, 12)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        let bytes = png.into_inner();
        let screenshot_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let hash = format!("{:x}", sha2::Sha256::digest(&bytes));
        let requirement = scenario.requirement_ids[0].clone();
        Ok(QaDriverOutputV1 {
            driver_version: request.driver_profile_version.clone(),
            completed: true,
            failure: None,
            display: QaDisplayV1 {
                width: 1024,
                height: 768,
                dpi: 96,
                theme: "light".into(),
                locale: "en-US".into(),
            },
            actions: vec![
                action(0, &scenario.id, "open", None, None),
                action(
                    1,
                    &scenario.id,
                    "screenshot",
                    Some(screenshot_id.clone()),
                    Some("view visible"),
                ),
            ],
            screenshots: vec![super::super::QaDriverScreenshotV1 {
                id: screenshot_id.clone(),
                scenario_id: scenario.id.clone(),
                sha256: hash,
                width: 16,
                height: 12,
                source_action: 1,
                redaction_masks: vec![],
                bytes,
            }],
            assertions: vec![super::super::QaAssertionV1 {
                id: uuid::Uuid::new_v4().hyphenated().to_string(),
                scenario_ids: vec![scenario.id.clone()],
                requirement_ids: vec![requirement],
                expected: "view visible".into(),
                observed: "view visible".into(),
                status: self.assertion_status,
                evidence_refs: vec![screenshot_id],
            }],
            logs: vec![],
            crashes: vec![],
            browser_console: vec![],
            failed_requests: vec![],
        })
    }
}

fn action(
    index: u32,
    scenario_id: &str,
    command: &str,
    evidence_id: Option<String>,
    observed: Option<&str>,
) -> QaActionEvidenceV1 {
    QaActionEvidenceV1 {
        index,
        scenario_id: scenario_id.into(),
        command: command.into(),
        args_redacted: Default::default(),
        pre_state: Some("initial".into()),
        post_state: observed
            .map(str::to_owned)
            .or_else(|| Some("loaded".into())),
        evidence_ids: evidence_id.into_iter().collect(),
        occurred_at_unix_ms: index as u64 + 1,
    }
}

fn fixture() -> (QaRunRequestV1, Vec<QaScenarioV1>, QaTaskServiceLeaseV1) {
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    let sha = || "a".repeat(64);
    let request = QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uid(),
        parent_task_uid: uid(),
        feature_uid: uid(),
        code_head_sha: sha(),
        build_artifact_sha256: sha(),
        complete_diff_hash: sha(),
        review_subject_sha256: sha(),
        approved_spec_hash: sha(),
        requirement_ids: vec!["REQ-1".into()],
        acceptance_criteria: vec!["View visible".into()],
        relevant_platforms: vec![QaPlatform::Web],
        startup_recipe_ref: "recipe:web".into(),
        startup_recipe_sha256: "e".repeat(64),
        execution_session_id: uid(),
        driver_profile_version: "web-driver/v1".into(),
        test_data_fixture_ref: None,
        bounded_limits: super::super::QaBoundedLimitsV1 {
            max_scenarios: 8,
            max_actions_per_scenario: 16,
            max_total_actions: 64,
            timeout_seconds: 120,
            max_evidence_bytes: 1024 * 1024,
            max_screenshots: 16,
        },
        author_run_id: uid(),
        author_session_id: uid(),
    };
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: uid(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec![],
        steps: vec![super::super::QaScenarioStepV1 {
            driver_action: super::super::QaDriverActionV1::BrowserOpen {
                url: "http://qa-service/".into(),
            },
            expected_state: Some("view visible".into()),
            screenshot_checkpoint: true,
        }],
        expected_results: vec!["View visible".into()],
        negative_paths: vec![],
        visual_expectations: vec![],
        priority: 1,
    };
    let lease = QaTaskServiceLeaseV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: request.parent_task_uid.clone(),
        task_container_id: "b".repeat(64),
        task_network_id: "c".repeat(64),
        preview_port: 4173,
        task_build_sha: request.build_artifact_sha256.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        startup_recipe_ref: request.startup_recipe_ref.clone(),
        startup_recipe_sha256: request.startup_recipe_sha256.clone(),
        internal_network_only: true,
        published_ports: false,
        expires_at_unix_ms: now_ms() + 60_000,
    };
    (request, vec![scenario], lease)
}

fn reviewer_prompt(
    request: &QaRunRequestV1,
    reviewer_run: &str,
    reviewer_session: &str,
) -> PromptManifest {
    render(&RoleInputV1 {
        role: PromptRole::RuntimeQaReviewer,
        workspace_uid: request.feature_uid.clone(),
        feature_uid: request.feature_uid.clone(),
        task_uid: Some(request.parent_task_uid.clone()),
        original_goal: "Review the application behavior".into(),
        approved_spec_ref: Some("approved-spec".into()),
        approved_spec_hash: Some(request.approved_spec_hash.clone()),
        decision_refs: vec![],
        evidence_manifest: Vec::<EvidenceItem>::new(),
        execution_policy_ref: "read-only isolated reviewer".into(),
        output_schema_version: "ReviewVerdictV1".into(),
        payload: RolePayloadV1::Review {
            subject_revision: "immutable-build".into(),
            subject_hash: request.code_head_sha.clone(),
            author_run_id: request.author_run_id.clone(),
            reviewer_run_id: reviewer_run.into(),
            author_session_id: Some(request.author_session_id.clone()),
            reviewer_session_id: Some(reviewer_session.into()),
            requirements_matrix: request.acceptance_criteria.clone(),
            explicit_exclusions: vec![],
        },
    })
    .unwrap()
    .manifest
}

#[test]
fn immutable_approved_run_persists_report_and_returns_gate_receipt() {
    let (request, scenarios, lease) = fixture();
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: QaAssertionStatus::Passed,
    };
    let (result, receipt) = RuntimeQaOrchestrator::new(&store)
        .execute(QaExecutionContext {
            request: &request,
            scenarios: &scenarios,
            authorization: QaTargetAuthorization::Web(&lease),
            driver: &mut driver,
            reviewer_execution_id: &reviewer_run,
            reviewer_session_id: &reviewer_session,
            prompt,
        })
        .unwrap();
    assert_eq!(result.outcome, QaOutcome::Pass);
    assert_eq!(
        receipt.as_ref().unwrap().subject_sha256,
        request.code_head_sha
    );
    assert_eq!(driver.calls, 1);
    assert_eq!(store.load_run(&request.qa_run_id).unwrap().3, result);
    assert_eq!(
        Some(store.load_gate_receipt(&request.qa_run_id).unwrap()),
        receipt
    );
}

#[test]
fn observed_failed_assertion_is_a_blocking_manager_finding() {
    let (request, scenarios, lease) = fixture();
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: QaAssertionStatus::Failed,
    };

    let (result, _) = RuntimeQaOrchestrator::new(&store)
        .execute(QaExecutionContext {
            request: &request,
            scenarios: &scenarios,
            authorization: QaTargetAuthorization::Web(&lease),
            driver: &mut driver,
            reviewer_execution_id: &reviewer_run,
            reviewer_session_id: &reviewer_session,
            prompt,
        })
        .unwrap();

    assert_eq!(result.outcome, QaOutcome::Fail);
    assert_eq!(result.findings.len(), 1);
    let finding = &result.findings[0];
    assert!(finding.blocking);
    assert_eq!(finding.requirement_uids, request.requirement_ids);
    assert_eq!(finding.screenshot_ids.len(), 1);
    assert!(!finding.trace_ids.is_empty());
    let stored = store.load_run(&request.qa_run_id).unwrap().3;
    assert_eq!(stored.findings, result.findings);
}
