use super::super::support::{id, role_receipt_for_author};
use koolade::{
    core::lifecycle::{GateDecision, GateReceipt, TaskRecord},
    harness::prompt_catalog::PromptRole,
    runtime_qa::{
        QaActionEvidenceV1, QaAssertionStatus, QaAssertionV1, QaBoundedLimitsV1, QaCaseResultV1,
        QaCaseStatus, QaCoverageStatus, QaCoverageV1, QaDisplayV1, QaDriverActionV1,
        QaEvidenceManifestV1, QaOutcome, QaPlatform, QaRequirementCoverageV1, QaResultV1,
        QaRunRequestV1, QaScenarioStepV1, QaScenarioV1, QaScreenshotArtifactV1, QaScreenshotV1,
        RuntimeQaStore,
    },
};
use sha2::Digest as _;
use std::{collections::BTreeMap, io::Cursor, path::Path};

/// Saves a schema-valid accepted browser-evidence fixture. It exercises the
/// persisted QA receipt contract only; it does not launch a browser or claim
/// interactive QA was performed.
pub(super) fn save_accepted_evidence_fixture(
    root: &Path,
    task: &TaskRecord,
) -> (QaRunRequestV1, GateReceipt) {
    let code_review = task.code_review.as_ref().expect("approved code review");
    let request = QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: id(),
        parent_task_uid: task.task_uid.clone(),
        feature_uid: task.feature_uid.clone(),
        code_head_sha: task.reviewed_code_head_sha.clone().unwrap(),
        build_artifact_sha256: "b".repeat(64),
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
        test_data_fixture_ref: Some("fixture:accepted-real-evidence-contract/v1".into()),
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
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: id(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["Accepted browser-evidence contract fixture".into()],
        steps: vec![QaScenarioStepV1 {
            driver_action: QaDriverActionV1::BrowserOpen {
                url: "http://qa-service/".into(),
            },
            expected_state: Some("Search dialog is available".into()),
            screenshot_checkpoint: true,
        }],
        expected_results: vec!["Keyboard dismissal evidence is accepted".into()],
        negative_paths: vec![],
        visual_expectations: vec!["Dialog content is visible".into()],
        priority: 1,
    };
    let screenshot_id = id();
    let mut image = Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(16, 12)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let bytes = image.into_inner();
    let screenshot_hash = format!("{:x}", sha2::Sha256::digest(&bytes));
    let scenario_id = scenario.id.clone();
    let evidence = QaEvidenceManifestV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: request.parent_task_uid.clone(),
        code_sha: request.code_head_sha.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        build_hash: request.build_artifact_sha256.clone(),
        platform: QaPlatform::Web,
        driver_version: request.driver_profile_version.clone(),
        display: QaDisplayV1 {
            width: 1024,
            height: 768,
            dpi: 96,
            theme: "light".into(),
            locale: "en-US".into(),
        },
        scenario_ids: vec![scenario_id.clone()],
        actions: vec![
            QaActionEvidenceV1 {
                index: 0,
                scenario_id: scenario_id.clone(),
                command: "open".into(),
                args_redacted: BTreeMap::new(),
                pre_state: Some("preview not loaded".into()),
                post_state: Some("accepted search dialog evidence loaded".into()),
                evidence_ids: Vec::new(),
                occurred_at_unix_ms: 1,
            },
            QaActionEvidenceV1 {
                index: 1,
                scenario_id: scenario_id.clone(),
                command: "screenshot".into(),
                args_redacted: BTreeMap::new(),
                pre_state: None,
                post_state: Some("accepted evidence reference captured".into()),
                evidence_ids: vec![screenshot_id.clone()],
                occurred_at_unix_ms: 2,
            },
        ],
        screenshots: vec![QaScreenshotV1 {
            id: screenshot_id.clone(),
            scenario_id: scenario_id.clone(),
            sha256: screenshot_hash,
            mime: "image/png".into(),
            width: 16,
            height: 12,
            source_action: 1,
            storage_ref: format!(
                "qa-evidence/{}/screenshots/{screenshot_id}.png",
                request.qa_run_id
            ),
            redaction_masks: Vec::new(),
        }],
        assertions: vec![QaAssertionV1 {
            id: id(),
            scenario_ids: vec![scenario_id.clone()],
            requirement_ids: request.requirement_ids.clone(),
            expected: "Keyboard dismissal is covered by accepted evidence".into(),
            observed: "Evidence contract references the approved interaction result".into(),
            status: QaAssertionStatus::Passed,
            evidence_refs: vec![screenshot_id.clone()],
        }],
        logs: Vec::new(),
        crashes: Vec::new(),
        browser_console: Vec::new(),
        failed_requests: Vec::new(),
        coverage: vec!["fixture:accepted-real-evidence-contract/v1".into()],
        retention_policy: "user-local/30-days".into(),
    };
    let mut receipt = role_receipt_for_author(
        PromptRole::RuntimeQaReviewerV2,
        GateDecision::Approved,
        &task.task_uid,
        &request.complete_diff_hash,
        &request.author_run_id,
        &request.author_session_id,
    );
    receipt.evidence_refs = vec![
        "fixture:accepted-real-evidence-contract/v1".into(),
        format!("runtime-qa:{}/result", request.qa_run_id),
        format!(
            "runtime-qa:{}/evidence:{}",
            request.qa_run_id,
            evidence.sha256().unwrap()
        ),
    ];
    let reviewer_execution_id = receipt.reviewer_run_id.clone().unwrap();
    let reviewer_session_id = receipt.reviewer_session_id.clone().unwrap();
    let result = QaResultV1 {
        schema_version: 1,
        subject_sha: request.code_head_sha.clone(),
        run_id: request.qa_run_id.clone(),
        reviewer_execution_id,
        reviewer_session_id,
        outcome: QaOutcome::Pass,
        evidence_manifest_sha256: evidence.sha256().unwrap(),
        cases: vec![QaCaseResultV1 {
            scenario_id: scenario_id.clone(),
            status: QaCaseStatus::Passed,
            evidence_refs: vec![screenshot_id.clone()],
            observations: vec!["accepted browser-evidence fixture".into()],
        }],
        coverage: QaCoverageV1 {
            requirements: vec![QaRequirementCoverageV1 {
                requirement_id: request.requirement_ids[0].clone(),
                status: QaCoverageStatus::Observed,
                evidence_refs: vec![screenshot_id.clone()],
            }],
            scenario_ids: vec![scenario_id],
            states: vec!["search-dialog-visible".into()],
        },
        findings: Vec::new(),
        tool_versions: [("driver".into(), request.driver_profile_version.clone())].into(),
        started_at_unix_ms: 1,
        completed_at_unix_ms: 2,
    };
    RuntimeQaStore::new(root.join("qa-records"))
        .save_run_with_screenshots(
            &request,
            &[scenario],
            &evidence,
            &result,
            &[QaScreenshotArtifactV1 {
                id: screenshot_id,
                bytes,
            }],
        )
        .unwrap();
    (request, receipt)
}
