#[path = "tests/multi_scenario.rs"]
mod multi_scenario;
#[path = "tests/orchestration.rs"]
mod orchestration;
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
    failure_before_actions: bool,
    failure_on_call: Option<usize>,
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
        if self.failure_before_actions || self.failure_on_call == Some(self.calls) {
            return Err(
                "private QA display did not start at /private/worker Authorization: Bearer abc123 token=secret"
                    .into(),
            );
        }
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            16,
            12,
            image::Rgba([240, 120, 60, 255]),
        ))
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
        planning_item_uid: None,
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
        dependency_cache_refs: Vec::new(),
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
        redaction_regions: vec![],
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
        role: PromptRole::RuntimeQaReviewerV2,
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
