use super::super::support::id;
use koolade::{
    core::lifecycle::{GateReceipt, LifecycleStore, TaskRecord},
    harness::prompt_catalog::{
        CoverageStatus, CoverageV1, PromptRole, ReviewVerdictV1, RoleInputV1, RolePayloadV1, render,
    },
    runtime_qa::{
        QaActionEvidenceV1, QaAssertionStatus, QaAssertionV1, QaBoundedLimitsV1, QaDisplayV1,
        QaDriverActionV1, QaDriverOutputV1, QaDriverScreenshotV1, QaEvidenceManifestV1, QaOutcome,
        QaPlatform, QaResultV1, QaRunRequestV1, QaScenarioDriver, QaScenarioStepV1, QaScenarioV1,
        QaTargetAuthorization, QaTaskServiceLeaseV1, RuntimeQaOrchestrator, RuntimeQaStore,
    },
};
use sha2::Digest as _;
use std::{collections::BTreeMap, io::Cursor, path::Path};

pub(super) fn execute_reviewed_run(
    root: &Path,
    lifecycle: &LifecycleStore<'_>,
    task: &mut TaskRecord,
    assertion_status: QaAssertionStatus,
) -> (QaRunRequestV1, QaOutcome, Option<GateReceipt>) {
    let code_review = task.code_review.as_ref().expect("approved code review");
    let request = QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: id(),
        parent_task_uid: task.task_uid.clone(),
        planning_item_uid: None,
        feature_uid: task.feature_uid.clone(),
        code_head_sha: task.reviewed_code_head_sha.clone().unwrap(),
        build_artifact_sha256: format!(
            "{:x}",
            sha2::Sha256::digest(task.implementation_sha256.as_deref().unwrap().as_bytes())
        ),
        complete_diff_hash: task.implementation_sha256.clone().unwrap(),
        review_subject_sha256: task.reviewed_code_subject_sha256.clone().unwrap(),
        approved_spec_hash: task.approved_plan_sha256.clone(),
        requirement_ids: vec!["REQ-001".into()],
        acceptance_criteria: vec!["Search can be dismissed from the keyboard".into()],
        relevant_platforms: vec![QaPlatform::Web],
        startup_recipe_ref: "recipe:fixture-web/v1".into(),
        startup_recipe_sha256: "c".repeat(64),
        execution_session_id: id(),
        driver_profile_version: "browser-driver/v1".into(),
        test_data_fixture_ref: Some("fixture:runtime-qa-lifecycle/v1".into()),
        dependency_cache_refs: Vec::new(),
        bounded_limits: QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 4,
            max_total_actions: 4,
            timeout_seconds: 60,
            max_evidence_bytes: 1024 * 1024,
            max_screenshots: 1,
        },
        author_run_id: code_review.author_run_id.clone().unwrap(),
        author_session_id: code_review.author_session_id.clone().unwrap(),
    };
    let queued = lifecycle
        .queue_runtime_qa(&task.task_uid, task.revision, &id(), request.clone())
        .unwrap();
    *task = queued;
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: id(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["Approved immutable web build is ready".into()],
        steps: vec![QaScenarioStepV1 {
            driver_action: QaDriverActionV1::BrowserOpen {
                url: "http://qa-service/".into(),
            },
            expected_state: Some("Search dialog is available".into()),
            screenshot_checkpoint: true,
        }],
        expected_results: vec!["Keyboard dismissal works".into()],
        negative_paths: vec![],
        visual_expectations: vec!["Dialog content is visible".into()],
        redaction_regions: vec![],
        priority: 1,
    };
    let lease = QaTaskServiceLeaseV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: task.task_uid.clone(),
        task_container_id: "a".repeat(64),
        task_network_id: "b".repeat(64),
        preview_port: 4173,
        task_build_sha: request.build_artifact_sha256.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        startup_recipe_ref: request.startup_recipe_ref.clone(),
        startup_recipe_sha256: request.startup_recipe_sha256.clone(),
        internal_network_only: true,
        published_ports: false,
        expires_at_unix_ms: now_ms() + 60_000,
    };
    let reviewer_execution_id = id();
    let reviewer_session_id = id();
    let prompt = render(&RoleInputV1 {
        role: PromptRole::RuntimeQaReviewerV2,
        workspace_uid: request.feature_uid.clone(),
        feature_uid: request.feature_uid.clone(),
        task_uid: Some(request.parent_task_uid.clone()),
        original_goal: request.acceptance_criteria.join("\n"),
        approved_spec_ref: None,
        approved_spec_hash: Some(request.approved_spec_hash.clone()),
        decision_refs: vec![],
        evidence_manifest: vec![],
        execution_policy_ref: "runtime-qa-review/read-only-v1".into(),
        output_schema_version: PromptRole::RuntimeQaReviewerV2
            .output_schema_version()
            .into(),
        payload: RolePayloadV1::Review {
            subject_revision: request.qa_run_id.clone(),
            subject_hash: request.code_head_sha.clone(),
            author_run_id: request.author_run_id.clone(),
            reviewer_run_id: reviewer_execution_id.clone(),
            author_session_id: Some(request.author_session_id.clone()),
            reviewer_session_id: Some(reviewer_session_id.clone()),
            requirements_matrix: request
                .requirement_ids
                .iter()
                .zip(&request.acceptance_criteria)
                .map(|(requirement, criterion)| format!("{requirement}: {criterion}"))
                .collect(),
            explicit_exclusions: vec![],
        },
    })
    .unwrap()
    .manifest;

    let store = RuntimeQaStore::new(root.join("qa-records"));
    let mut driver = LifecycleFixtureDriver { assertion_status };
    let mut reviewer = |evidence: &QaEvidenceManifestV1, _candidate: &QaResultV1| {
        Ok(ReviewVerdictV1 {
            subject_hash: evidence.sha256()?,
            examined_files: Vec::new(),
            tested_evidence_ids: Vec::new(),
            unexamined_areas: Vec::new(),
            pass: true,
            outcome: Some(koolade::harness::prompt_catalog::ReviewOutcomeV1::Pass),
            coverage: vec![CoverageV1 {
                requirement: request.requirement_ids[0].clone(),
                status: CoverageStatus::Pass,
                evidence: vec![evidence.actions[0].index.to_string()],
            }],
            findings: vec![],
            untested: vec![],
        })
    };
    let (result, receipt) = RuntimeQaOrchestrator::new(&store)
        .execute_reviewed(
            koolade::runtime_qa::QaExecutionContext {
                request: &request,
                scenarios: &[scenario],
                authorization: QaTargetAuthorization::Web(&lease),
                driver: &mut driver,
                reviewer_execution_id: &reviewer_execution_id,
                reviewer_session_id: &reviewer_session_id,
                prompt,
            },
            &mut reviewer,
        )
        .unwrap();
    (request, result.outcome, receipt)
}

struct LifecycleFixtureDriver {
    assertion_status: QaAssertionStatus,
}

impl QaScenarioDriver for LifecycleFixtureDriver {
    fn platform(&self) -> QaPlatform {
        QaPlatform::Web
    }

    fn run_scenario(
        &mut self,
        request: &QaRunRequestV1,
        scenario: &QaScenarioV1,
        _authorization: &QaTargetAuthorization<'_>,
    ) -> Result<QaDriverOutputV1, String> {
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(16, 12)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        let bytes = png.into_inner();
        let screenshot_id = id();
        let screenshot_hash = format!("{:x}", sha2::Sha256::digest(&bytes));
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
                action(0, &scenario.id, "open", None),
                action(1, &scenario.id, "screenshot", Some(screenshot_id.clone())),
            ],
            screenshots: vec![QaDriverScreenshotV1 {
                id: screenshot_id.clone(),
                scenario_id: scenario.id.clone(),
                sha256: screenshot_hash,
                width: 16,
                height: 12,
                source_action: 1,
                redaction_masks: vec![],
                bytes,
            }],
            assertions: vec![QaAssertionV1 {
                id: id(),
                scenario_ids: vec![scenario.id.clone()],
                requirement_ids: scenario.requirement_ids.clone(),
                expected: "Keyboard dismissal works".into(),
                observed: "Keyboard dismissal observed".into(),
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
    evidence: Option<String>,
) -> QaActionEvidenceV1 {
    QaActionEvidenceV1 {
        index,
        scenario_id: scenario_id.into(),
        command: command.into(),
        args_redacted: BTreeMap::new(),
        pre_state: Some("fixture page loaded".into()),
        post_state: Some("fixture interaction recorded".into()),
        evidence_ids: evidence.into_iter().collect(),
        occurred_at_unix_ms: u64::from(index) + 1,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
