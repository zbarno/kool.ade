use std::{io::Cursor, path::Path, time::SystemTime};

use crate::{
    core::lifecycle::{LifecycleStore, TaskRecord},
    harness::prompt_catalog::{
        CoverageStatus, CoverageV1, PromptRole, ReviewOutcomeV1, ReviewVerdictV1, RoleInputV1,
        RolePayloadV1, render,
    },
    runtime_qa::*,
};
use sha2::Digest as _;

pub(super) fn run_and_record(
    lifecycle: &LifecycleStore<'_>,
    task: &TaskRecord,
    request: &QaRunRequestV1,
    root: &Path,
) -> TaskRecord {
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: id(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["The approved web target is available".into()],
        steps: vec![QaScenarioStepV1 {
            driver_action: QaDriverActionV1::BrowserOpen {
                url: "http://qa-service/".into(),
            },
            expected_state: Some("The approved page is visible".into()),
            screenshot_checkpoint: true,
        }],
        expected_results: vec!["The approved page is visible".into()],
        negative_paths: Vec::new(),
        visual_expectations: vec!["The page content is visible".into()],
        redaction_regions: Vec::new(),
        priority: 1,
    };
    let authorization = QaTaskServiceLeaseV1 {
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
    let prompt = reviewer_prompt(request, &reviewer_execution_id, &reviewer_session_id);
    let qa_store = RuntimeQaStore::new(root.join("qa"));
    let mut driver = FixtureDriver;
    let mut reviewer = |evidence: &QaEvidenceManifestV1,
                        candidate: &QaResultV1,
                        screenshots: &[QaScreenshotArtifactV1]| {
        Ok(ReviewVerdictV1 {
            subject_hash: crate::runtime_qa::qa_review_subject_sha256(
                request, candidate, evidence,
            )?,
            examined_files: Vec::new(),
            tested_evidence_ids: screenshots
                .iter()
                .map(|screenshot| screenshot.id.clone())
                .collect(),
            unexamined_areas: Vec::new(),
            pass: true,
            outcome: Some(ReviewOutcomeV1::Pass),
            coverage: vec![CoverageV1 {
                requirement: request.requirement_ids[0].clone(),
                status: CoverageStatus::Pass,
                evidence: vec![
                    evidence.actions[0].index.to_string(),
                    screenshots[0].id.clone(),
                ],
            }],
            findings: Vec::new(),
            untested: Vec::new(),
        })
    };
    let (result, _receipt) = RuntimeQaOrchestrator::new(&qa_store)
        .execute_reviewed(
            QaExecutionContext {
                request,
                scenarios: &[scenario],
                authorization: QaTargetAuthorization::Web(&authorization),
                driver: &mut driver,
                reviewer_execution_id: &reviewer_execution_id,
                reviewer_session_id: &reviewer_session_id,
                prompt,
            },
            &mut reviewer,
        )
        .unwrap();
    assert_eq!(result.outcome, QaOutcome::Pass);
    lifecycle
        .record_runtime_qa_report(&task.task_uid, task.revision, &id(), &qa_store)
        .unwrap()
}

struct FixtureDriver;

impl QaScenarioDriver for FixtureDriver {
    fn platform(&self) -> QaPlatform {
        QaPlatform::Web
    }

    fn run_scenario(
        &mut self,
        request: &QaRunRequestV1,
        scenario: &QaScenarioV1,
        authorization: &QaTargetAuthorization<'_>,
    ) -> Result<QaDriverOutputV1, String> {
        let QaTargetAuthorization::Web(lease) = authorization else {
            return Err("fixture browser driver received a non-web authorization".into());
        };
        if lease.task_build_sha != request.build_artifact_sha256
            || lease.startup_recipe_ref != request.startup_recipe_ref
            || lease.startup_recipe_sha256 != request.startup_recipe_sha256
        {
            return Err("fixture browser authorization does not match its request".into());
        }
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(8, 8)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        let bytes = png.into_inner();
        let screenshot_id = id();
        let screenshot_sha256 = format!("{:x}", sha2::Sha256::digest(&bytes));
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
            actions: vec![QaActionEvidenceV1 {
                index: 0,
                scenario_id: scenario.id.clone(),
                command: "screenshot".into(),
                args_redacted: Default::default(),
                pre_state: Some("web target started".into()),
                post_state: Some("approved page is visible".into()),
                evidence_ids: vec![screenshot_id.clone()],
                occurred_at_unix_ms: now_ms(),
            }],
            screenshots: vec![QaDriverScreenshotV1 {
                id: screenshot_id.clone(),
                scenario_id: scenario.id.clone(),
                sha256: screenshot_sha256,
                width: 8,
                height: 8,
                source_action: 0,
                redaction_masks: Vec::new(),
                bytes,
            }],
            assertions: vec![QaAssertionV1 {
                id: id(),
                scenario_ids: vec![scenario.id.clone()],
                requirement_ids: scenario.requirement_ids.clone(),
                expected: "approved page is visible".into(),
                observed: "approved page is visible".into(),
                status: QaAssertionStatus::Passed,
                evidence_refs: vec![screenshot_id],
            }],
            logs: Vec::new(),
            crashes: Vec::new(),
            browser_console: Vec::new(),
            failed_requests: Vec::new(),
        })
    }
}

fn reviewer_prompt(
    request: &QaRunRequestV1,
    reviewer_execution_id: &str,
    reviewer_session_id: &str,
) -> crate::harness::prompt_catalog::PromptManifest {
    render(&RoleInputV1 {
        role: PromptRole::RuntimeQaReviewerV3,
        workspace_uid: "fixture-workspace".into(),
        feature_uid: request.feature_uid.clone(),
        task_uid: Some(request.parent_task_uid.clone()),
        original_goal: request.acceptance_criteria.join("\n"),
        approved_spec_ref: Some(request.feature_uid.clone()),
        approved_spec_hash: Some(request.approved_spec_hash.clone()),
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "runtime-qa-projection-fixture/v1".into(),
        output_schema_version: PromptRole::RuntimeQaReviewerV3
            .output_schema_version()
            .into(),
        payload: RolePayloadV1::Review {
            subject_revision: request.code_head_sha.clone(),
            subject_hash: request.complete_diff_hash.clone(),
            author_run_id: request.author_run_id.clone(),
            reviewer_run_id: reviewer_execution_id.into(),
            author_session_id: Some(request.author_session_id.clone()),
            reviewer_session_id: Some(reviewer_session_id.into()),
            requirements_matrix: request.requirement_ids.clone(),
            explicit_exclusions: Vec::new(),
        },
    })
    .unwrap()
    .manifest
}

fn id() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
