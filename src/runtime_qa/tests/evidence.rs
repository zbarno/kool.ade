use super::{request, scenario};
use crate::runtime_qa::*;
use std::io::Cursor;

use sha2::Digest as _;

struct ReportFixture {
    request: QaRunRequestV1,
    scenarios: Vec<QaScenarioV1>,
    evidence: QaEvidenceManifestV1,
    result: QaResultV1,
    screenshot: QaScreenshotArtifactV1,
}

fn report_fixture() -> ReportFixture {
    let request = request();
    let scenario = scenario(&request);
    let screenshot_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut image = Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(16, 12)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let bytes = image.into_inner();
    let hash = format!("{:x}", sha2::Sha256::digest(&bytes));
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
                command: "navigate_allowed_url".into(),
                args_redacted: Default::default(),
                pre_state: Some("preview not loaded".into()),
                post_state: Some("home view loaded".into()),
                evidence_ids: vec![],
                occurred_at_unix_ms: 10,
            },
            QaActionEvidenceV1 {
                index: 1,
                scenario_id: scenario_id.clone(),
                command: "screenshot".into(),
                args_redacted: Default::default(),
                pre_state: None,
                post_state: Some("home view visible".into()),
                evidence_ids: vec![screenshot_id.clone()],
                occurred_at_unix_ms: 11,
            },
        ],
        screenshots: vec![QaScreenshotV1 {
            id: screenshot_id.clone(),
            scenario_id: scenario_id.clone(),
            sha256: hash,
            mime: "image/png".into(),
            width: 16,
            height: 12,
            source_action: 1,
            storage_ref: format!(
                "qa-evidence/{}/screenshots/{screenshot_id}.png",
                request.qa_run_id
            ),
            redaction_masks: vec![],
        }],
        assertions: vec![QaAssertionV1 {
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            scenario_ids: vec![scenario_id.clone()],
            requirement_ids: request.requirement_ids.clone(),
            expected: "The main view renders".into(),
            observed: "The home view was visible after navigation".into(),
            status: QaAssertionStatus::Passed,
            evidence_refs: vec![screenshot_id.clone()],
        }],
        logs: vec![],
        crashes: vec![],
        browser_console: vec![],
        failed_requests: vec![],
        coverage: request.requirement_ids.clone(),
        retention_policy: "user-local/30-days".into(),
    };
    let result = QaResultV1 {
        schema_version: 1,
        subject_sha: request.code_head_sha.clone(),
        run_id: request.qa_run_id.clone(),
        reviewer_execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        reviewer_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        outcome: QaOutcome::Pass,
        evidence_manifest_sha256: evidence.sha256().unwrap(),
        cases: vec![QaCaseResultV1 {
            scenario_id: scenario_id.clone(),
            status: QaCaseStatus::Passed,
            evidence_refs: vec![screenshot_id.clone()],
            observations: vec!["home view visible".into()],
        }],
        coverage: QaCoverageV1 {
            requirements: vec![QaRequirementCoverageV1 {
                requirement_id: request.requirement_ids[0].clone(),
                status: QaCoverageStatus::Observed,
                evidence_refs: vec![screenshot_id.clone()],
            }],
            scenario_ids: vec![scenario_id],
            states: vec!["home".into()],
        },
        findings: vec![],
        tool_versions: [("driver".into(), request.driver_profile_version.clone())].into(),
        started_at_unix_ms: 1,
        completed_at_unix_ms: 12,
    };
    ReportFixture {
        request,
        scenarios: vec![scenario],
        evidence,
        result,
        screenshot: QaScreenshotArtifactV1 {
            id: screenshot_id,
            bytes,
        },
    }
}

#[test]
fn report_is_bound_to_exact_code_build_spec_and_driver() {
    let fixture = report_fixture();
    assert!(
        validate_report(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result
        )
        .is_ok()
    );

    let mut stale = fixture.request.clone();
    stale.code_head_sha = "f".repeat(64);
    assert!(
        fixture
            .evidence
            .validate(&stale, &fixture.scenarios)
            .is_err()
    );
    stale = fixture.request.clone();
    stale.approved_spec_hash = "e".repeat(64);
    assert!(
        fixture
            .evidence
            .validate(&stale, &fixture.scenarios)
            .is_err()
    );
    stale = fixture.request.clone();
    stale.build_artifact_sha256 = "d".repeat(64);
    assert!(
        fixture
            .evidence
            .validate(&stale, &fixture.scenarios)
            .is_err()
    );
    stale = fixture.request.clone();
    stale.driver_profile_version = "driver/v2".into();
    assert!(
        fixture
            .evidence
            .validate(&stale, &fixture.scenarios)
            .is_err()
    );
}

#[path = "evidence/dispositions.rs"]
mod dispositions;
#[path = "evidence/git_object_ids.rs"]
mod git_object_ids;

#[test]
fn missing_or_forged_screenshot_bytes_are_rejected_and_load_rechecks_hashes() {
    let fixture = report_fixture();
    assert!(
        validate_screenshot_artifact(
            &fixture.evidence.screenshots[0],
            &[],
            fixture.request.bounded_limits.max_evidence_bytes
        )
        .is_err()
    );
    let mut forged = fixture.screenshot.bytes.clone();
    forged[30] ^= 0xff;
    assert!(
        validate_screenshot_artifact(
            &fixture.evidence.screenshots[0],
            &forged,
            fixture.request.bounded_limits.max_evidence_bytes
        )
        .is_err()
    );

    let root = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(root.path().join("qa"));
    assert!(
        store
            .save_run_with_screenshots(
                &fixture.request,
                &fixture.scenarios,
                &fixture.evidence,
                &fixture.result,
                &[],
            )
            .is_err()
    );
    store
        .save_run_with_screenshots(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result,
            std::slice::from_ref(&fixture.screenshot),
        )
        .unwrap();
    assert!(store.load_run(&fixture.request.qa_run_id).is_ok());
    assert_eq!(
        store
            .load_screenshot(
                &fixture.request.qa_run_id,
                &fixture.evidence.screenshots[0].id,
            )
            .unwrap()
            .bytes,
        fixture.screenshot.bytes
    );
    let screenshots = store.load_screenshots(&fixture.request.qa_run_id).unwrap();
    assert_eq!(screenshots.len(), 1);
    assert_eq!(screenshots[0].id, fixture.screenshot.id);
    assert_eq!(screenshots[0].bytes, fixture.screenshot.bytes);
    assert!(
        store
            .load_screenshot(&fixture.request.qa_run_id, "unknown-screenshot")
            .is_err()
    );

    let artifact_path = store
        .root()
        .join(&fixture.evidence.screenshots[0].storage_ref);
    std::fs::write(artifact_path, b"tampered PNG").unwrap();
    assert!(store.load_run(&fixture.request.qa_run_id).is_err());
}

#[test]
fn incomplete_coverage_and_failed_assertions_never_pass() {
    let mut fixture = report_fixture();
    fixture.evidence.assertions.clear();
    assert!(
        fixture
            .evidence
            .validate(&fixture.request, &fixture.scenarios)
            .is_err()
    );

    fixture = report_fixture();
    fixture.evidence.assertions[0].status = QaAssertionStatus::Failed;
    fixture.evidence.assertions[0].observed = "The expected view did not appear".into();
    fixture.result.evidence_manifest_sha256 = fixture.evidence.sha256().unwrap();
    assert!(
        validate_report(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result
        )
        .is_err()
    );

    fixture.result.outcome = QaOutcome::Fail;
    fixture.result.cases[0].status = QaCaseStatus::Failed;
    assert!(
        validate_report(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result
        )
        .is_ok()
    );
}
