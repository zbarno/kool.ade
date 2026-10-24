use std::io::Cursor;

use crate::{
    core::lifecycle::{GateActor, GateDecision, LifecycleStore, TaskRecord},
    runtime_qa::{
        CURRENT_QA_RESULT_SCHEMA_VERSION, QaActionEvidenceV1, QaAssertionStatus, QaAssertionV1,
        QaCaseResultV1, QaCaseStatus, QaCoverageStatus, QaCoverageV1, QaDisplayV1,
        QaDriverActionV1, QaEvidenceManifestV1, QaOutcome, QaPlatform, QaRequirementCoverageV1,
        QaResultV1, QaReviewCoverageStatusV1, QaReviewRequirementCoverageV1, QaRunRequestV1,
        QaScenarioStepV1, QaScenarioV1, QaScreenshotArtifactV1, QaScreenshotV1, RuntimeQaStore,
    },
};
use sha2::Digest as _;

pub(super) fn record_completed_qa(
    lifecycle: &LifecycleStore<'_>,
    task: &TaskRecord,
    request: &QaRunRequestV1,
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
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(8, 8)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let screenshot_bytes = png.into_inner();
    let screenshot_id = id();
    let screenshot_sha256 = format!("{:x}", sha2::Sha256::digest(&screenshot_bytes));
    let evidence = QaEvidenceManifestV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: task.task_uid.clone(),
        target_id: request.target_id.clone(),
        code_sha: request.code_head_sha.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        build_hash: request.build_artifact_sha256.clone(),
        platform: QaPlatform::Web,
        driver_version: request.driver_profile_version.clone(),
        display: QaDisplayV1 {
            width: Some(1024),
            height: Some(768),
            dpi: Some(96),
            theme: "light".into(),
            locale: "en-US".into(),
        },
        scenario_ids: vec![scenario.id.clone()],
        actions: vec![QaActionEvidenceV1 {
            index: 0,
            scenario_id: scenario.id.clone(),
            command: "screenshot".into(),
            args_redacted: Default::default(),
            pre_state: Some("web target started".into()),
            post_state: Some("approved page is visible".into()),
            evidence_ids: vec![screenshot_id.clone()],
            occurred_at_unix_ms: 2,
        }],
        screenshots: vec![QaScreenshotV1 {
            id: screenshot_id.clone(),
            scenario_id: scenario.id.clone(),
            sha256: screenshot_sha256,
            mime: "image/png".into(),
            width: 8,
            height: 8,
            source_action: 0,
            storage_ref: format!(
                "qa-evidence/{}/screenshots/{screenshot_id}.png",
                request.qa_run_id
            ),
            redaction_masks: Vec::new(),
        }],
        assertions: request
            .requirement_ids
            .iter()
            .map(|requirement_id| QaAssertionV1 {
                id: id(),
                scenario_ids: vec![scenario.id.clone()],
                requirement_ids: vec![requirement_id.clone()],
                expected: "approved page is visible".into(),
                observed: "approved page is visible".into(),
                status: QaAssertionStatus::Passed,
                evidence_refs: vec![screenshot_id.clone()],
            })
            .collect(),
        logs: Vec::new(),
        crashes: Vec::new(),
        browser_console: Vec::new(),
        failed_requests: Vec::new(),
        coverage: request.requirement_ids.clone(),
        retention_policy: "user-local/30-days".into(),
    };
    let evidence_hash = evidence.sha256().unwrap();
    let reviewer_execution_id = id();
    let reviewer_session_id = id();
    let result = QaResultV1 {
        schema_version: CURRENT_QA_RESULT_SCHEMA_VERSION,
        subject_sha: request.code_head_sha.clone(),
        run_id: request.qa_run_id.clone(),
        reviewer_execution_id: reviewer_execution_id.clone(),
        reviewer_session_id: reviewer_session_id.clone(),
        outcome: QaOutcome::Pass,
        evidence_manifest_sha256: evidence_hash.clone(),
        scenario_set_sha256: crate::runtime_qa::scenario_set_sha256(std::slice::from_ref(
            &scenario,
        ))
        .unwrap(),
        reviewed_screenshot_ids: vec![screenshot_id.clone()],
        review_coverage: request
            .requirement_ids
            .iter()
            .map(|requirement_id| QaReviewRequirementCoverageV1 {
                requirement_id: requirement_id.clone(),
                status: QaReviewCoverageStatusV1::Pass,
                evidence_refs: vec![screenshot_id.clone(), "0".into()],
            })
            .collect(),
        cases: vec![QaCaseResultV1 {
            scenario_id: scenario.id.clone(),
            status: QaCaseStatus::Passed,
            evidence_refs: vec![screenshot_id.clone()],
            observations: vec!["approved page is visible".into()],
        }],
        coverage: QaCoverageV1 {
            requirements: request
                .requirement_ids
                .iter()
                .map(|requirement_id| QaRequirementCoverageV1 {
                    requirement_id: requirement_id.clone(),
                    status: QaCoverageStatus::Observed,
                    evidence_refs: vec![screenshot_id.clone()],
                })
                .collect(),
            scenario_ids: vec![scenario.id.clone()],
            states: vec!["home".into()],
        },
        findings: Vec::new(),
        tool_versions: [("driver".into(), request.driver_profile_version.clone())].into(),
        started_at_unix_ms: 1,
        completed_at_unix_ms: 3,
    };
    let temp = tempfile::tempdir().unwrap();
    let qa_store = RuntimeQaStore::new(temp.path().join("qa"));
    qa_store
        .save_run_with_screenshots(
            request,
            std::slice::from_ref(&scenario),
            &evidence,
            &result,
            &[QaScreenshotArtifactV1 {
                id: screenshot_id,
                bytes: screenshot_bytes,
            }],
        )
        .unwrap();
    let mut receipt = super::super::super::super::receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &task.task_uid,
    );
    super::super::super::super::set_prompt_role(
        &mut receipt,
        crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV3,
    );
    receipt.subject_sha256 = request.complete_diff_hash.clone();
    receipt.author_run_id = Some(request.author_run_id.clone());
    receipt.author_session_id = Some(request.author_session_id.clone());
    receipt.reviewer_run_id = Some(reviewer_execution_id);
    receipt.reviewer_session_id = Some(reviewer_session_id);
    receipt.evidence_refs = vec![
        format!("runtime-qa:{}/result", request.qa_run_id),
        format!("runtime-qa:{}/evidence:{evidence_hash}", request.qa_run_id),
        format!(
            "runtime-qa:{}/scenarios:{}",
            request.qa_run_id, result.scenario_set_sha256
        ),
    ];
    qa_store.save_gate_receipt(&receipt).unwrap();
    lifecycle
        .record_runtime_qa_report(&task.task_uid, task.revision, &id(), &qa_store)
        .unwrap()
}

fn id() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}
