use super::super::support::id;
use koolade::{
    core::lifecycle::{GateReceipt, LifecycleStore, TaskEvent, TaskRecord},
    harness::prompt_catalog::{
        CoverageStatus, CoverageV1, PromptManifest, PromptRole, ReviewVerdictV1, RoleInputV1,
        RolePayloadV1, render,
    },
    runtime_qa::{
        QaAssertionStatus, QaAssertionV1, QaBoundedLimitsV1, QaDisplayV1, QaDriverActionV1,
        QaDriverOutputV1, QaDriverScreenshotV1, QaEvidenceManifestV1, QaOutcome, QaPlatform,
        QaResultV1, QaRunRequestV1, QaScenarioDriver, QaScenarioStepV1, QaScenarioV1,
        QaTargetAuthorization, QaTaskServiceLeaseV1, RuntimeQaOrchestrator, RuntimeQaStore,
    },
};
use sha2::Digest as _;
use std::{io::Cursor, path::Path};

#[path = "qa_report/helpers.rs"]
mod helpers;
use helpers::{action, now_ms};

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
        target_id: Some("fixture-target".into()),
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
    // Production boundary: the public lifecycle store rejects a new targetless
    // QA event while reducer replay remains compatible with historical events.
    // This uses temporary state and deterministic drivers/reviewers only; no
    // Docker, SSH, network, or credentials, with moderate in-process setup cost.
    let mut unbound_request = request.clone();
    unbound_request.target_id = None;
    assert!(
        lifecycle
            .apply_task_event(
                &task.task_uid,
                task.revision,
                &id(),
                TaskEvent::RuntimeQaRequested(unbound_request),
            )
            .is_err()
    );
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
        // This lifecycle fixture checks functional coverage; pixel review has
        // separate orchestration and MCP boundary tests.
        // This synthetic lifecycle fixture proves typed interaction and report
        // flow; visual claims require a separate image-review receipt.
        visual_expectations: Vec::new(),
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
    let prompt = reviewer_prompt(
        &request,
        &reviewer_execution_id,
        &reviewer_session_id,
        PromptRole::RuntimeQaReviewerV3,
    );

    let store = RuntimeQaStore::new(root.join("qa-records"));
    let mut driver = LifecycleFixtureDriver { assertion_status };
    let mut reviewer =
        |evidence: &QaEvidenceManifestV1,
         candidate: &QaResultV1,
         screenshots: &[koolade::runtime_qa::QaScreenshotArtifactV1]| {
            Ok(ReviewVerdictV1 {
                subject_hash: koolade::runtime_qa::qa_review_subject_sha256(
                    &request, candidate, evidence,
                )?,
                examined_files: Vec::new(),
                tested_evidence_ids: screenshots
                    .iter()
                    .map(|screenshot| screenshot.id.clone())
                    .collect(),
                unexamined_areas: Vec::new(),
                pass: candidate.outcome == QaOutcome::Pass,
                outcome: Some(if candidate.outcome == QaOutcome::Pass {
                    koolade::harness::prompt_catalog::ReviewOutcomeV1::Pass
                } else {
                    koolade::harness::prompt_catalog::ReviewOutcomeV1::BlockingFindings
                }),
                coverage: vec![CoverageV1 {
                    requirement: request.requirement_ids[0].clone(),
                    status: CoverageStatus::Pass,
                    evidence: screenshots
                        .iter()
                        .map(|screenshot| screenshot.id.clone())
                        .chain([evidence.actions[0].index.to_string()])
                        .collect(),
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

pub(super) fn reviewer_prompt(
    request: &QaRunRequestV1,
    reviewer_execution_id: &str,
    reviewer_session_id: &str,
    role: PromptRole,
) -> PromptManifest {
    render(&RoleInputV1 {
        role,
        workspace_uid: request.feature_uid.clone(),
        feature_uid: request.feature_uid.clone(),
        task_uid: Some(request.parent_task_uid.clone()),
        original_goal: request.acceptance_criteria.join("\n"),
        approved_spec_ref: None,
        approved_spec_hash: Some(request.approved_spec_hash.clone()),
        decision_refs: vec![],
        evidence_manifest: vec![],
        execution_policy_ref: "runtime-qa-review/read-only-v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: request.qa_run_id.clone(),
            subject_hash: request.code_head_sha.clone(),
            author_run_id: request.author_run_id.clone(),
            reviewer_run_id: reviewer_execution_id.into(),
            author_session_id: Some(request.author_session_id.clone()),
            reviewer_session_id: Some(reviewer_session_id.into()),
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
    .manifest
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
                width: Some(1024),
                height: Some(768),
                dpi: Some(96),
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
