use super::*;
use crate::harness::prompt_catalog::{CoverageStatus, CoverageV1, ReviewVerdictV1};

#[test]
// Covers the persisted report/gate-receipt compatibility boundary: old
// schema-1 and schema-2 passing reports remain viewable but cannot create a new approval.
// A validator-only test cannot prove JSON defaults or receipt lookup after
// persistence. The existing receipt and screenshot files use a temporary
// local store, with no external services and low setup cost.
fn immutable_approved_run_persists_report_and_returns_gate_receipt() {
    let (request, mut scenarios, lease) = fixture();
    scenarios[0].redaction_regions = vec![crate::runtime_qa::QaRedactionRegionV1 {
        x: 2,
        y: 3,
        width: 4,
        height: 2,
    }];
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: QaAssertionStatus::Passed,
        failure_before_actions: false,
        failure_on_call: None,
    };
    let mut reviewer = |evidence: &QaEvidenceManifestV1,
                        candidate: &QaResultV1,
                        screenshots: &[QaScreenshotArtifactV1]| {
        Ok(ReviewVerdictV1 {
            subject_hash: crate::runtime_qa::qa_review_subject_sha256(
                &request, candidate, evidence,
            )?,
            examined_files: Vec::new(),
            tested_evidence_ids: vec![screenshots[0].id.clone()],
            unexamined_areas: Vec::new(),
            pass: true,
            outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass),
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
    let (result, receipt) = RuntimeQaOrchestrator::new(&store)
        .execute_reviewed(
            QaExecutionContext {
                request: &request,
                scenarios: &scenarios,
                authorization: QaTargetAuthorization::Web(&lease),
                driver: &mut driver,
                reviewer_execution_id: &reviewer_run,
                reviewer_session_id: &reviewer_session,
                prompt,
            },
            &mut reviewer,
        )
        .unwrap();
    assert_eq!(result.outcome, QaOutcome::Pass);
    assert_eq!(
        receipt.as_ref().unwrap().subject_sha256,
        request.code_head_sha
    );
    assert_eq!(driver.calls, 1);
    assert_eq!(store.load_run(&request.qa_run_id).unwrap().3, result);
    let persisted = store.load_run_with_screenshots(&request.qa_run_id).unwrap();
    assert_eq!(
        persisted.evidence.screenshots[0].redaction_masks,
        ["rect:2,3,4,2"]
    );
    let image = image::load_from_memory(&persisted.screenshots[0].bytes)
        .unwrap()
        .to_rgba8();
    assert_eq!(image.get_pixel(2, 3), &image::Rgba([0, 0, 0, 255]));
    assert_eq!(image.get_pixel(5, 4), &image::Rgba([0, 0, 0, 255]));
    assert_eq!(image.get_pixel(0, 0), &image::Rgba([240, 120, 60, 255]));
    assert_eq!(
        persisted.evidence.screenshots[0].sha256,
        format!(
            "{:x}",
            sha2::Sha256::digest(&persisted.screenshots[0].bytes)
        )
    );
    assert_eq!(
        Some(store.load_gate_receipt(&request.qa_run_id).unwrap()),
        receipt
    );

    let mut legacy_v3_json = serde_json::to_value(&result).unwrap();
    legacy_v3_json["schema_version"] = serde_json::json!(3);
    legacy_v3_json
        .as_object_mut()
        .unwrap()
        .remove("scenario_set_sha256");
    let legacy_v3: QaResultV1 = serde_json::from_value(legacy_v3_json.clone()).unwrap();
    let mut legacy_v3_receipt = receipt.as_ref().unwrap().clone();
    legacy_v3_receipt
        .evidence_refs
        .retain(|reference| !reference.contains("/scenarios:"));
    std::fs::write(
        store.root().join(&request.qa_run_id).join("result.json"),
        serde_json::to_vec(&legacy_v3_json).unwrap(),
    )
    .unwrap();
    std::fs::write(
        store
            .root()
            .join(&request.qa_run_id)
            .join("gate-receipt.json"),
        serde_json::to_vec(&legacy_v3_receipt).unwrap(),
    )
    .unwrap();
    assert_eq!(store.load_run(&request.qa_run_id).unwrap().3, legacy_v3);
    assert_eq!(
        store.load_gate_receipt(&request.qa_run_id).unwrap(),
        legacy_v3_receipt
    );
    assert!(store.load_current_run(&request.qa_run_id).is_err());
    std::fs::write(
        store.root().join(&request.qa_run_id).join("result.json"),
        serde_json::to_vec(&result).unwrap(),
    )
    .unwrap();
    std::fs::write(
        store
            .root()
            .join(&request.qa_run_id)
            .join("gate-receipt.json"),
        serde_json::to_vec(receipt.as_ref().unwrap()).unwrap(),
    )
    .unwrap();

    let mut legacy_v2_json = serde_json::to_value(&result).unwrap();
    let legacy_v2_object = legacy_v2_json.as_object_mut().unwrap();
    legacy_v2_object.remove("reviewed_screenshot_ids");
    legacy_v2_object.remove("review_coverage");
    legacy_v2_object.insert("schema_version".into(), serde_json::json!(2));
    let legacy_v2: QaResultV1 = serde_json::from_value(legacy_v2_json).unwrap();
    crate::runtime_qa::validate_report(&request, &scenarios, &persisted.evidence, &legacy_v2)
        .unwrap();
    assert!(
        crate::runtime_qa::validate_current_report(
            &request,
            &scenarios,
            &persisted.evidence,
            &legacy_v2,
        )
        .is_err()
    );
    let result_path = store.root().join(&request.qa_run_id).join("result.json");
    std::fs::write(&result_path, serde_json::to_vec(&legacy_v2).unwrap()).unwrap();
    assert_eq!(store.load_run(&request.qa_run_id).unwrap().3, legacy_v2);
    assert!(store.load_current_run(&request.qa_run_id).is_err());
    std::fs::write(&result_path, serde_json::to_vec(&result).unwrap()).unwrap();
    let mut legacy_v2_receipt = receipt.as_ref().unwrap().clone();
    legacy_v2_receipt
        .evidence_refs
        .retain(|reference| !reference.contains("/scenarios:"));
    let legacy_v2_prompt = reviewer_prompt_for_role(
        &request,
        &reviewer_run,
        &reviewer_session,
        PromptRole::RuntimeQaReviewerV2,
    );
    legacy_v2_receipt.prompt_sha256 = Some(legacy_v2_prompt.identity_sha256());
    legacy_v2_receipt.prompt_manifest = Some(legacy_v2_prompt);
    assert!(legacy_v2_receipt.validate().is_ok());
    std::fs::write(
        store.root().join(&request.qa_run_id).join("result.json"),
        serde_json::to_vec(&legacy_v2).unwrap(),
    )
    .unwrap();
    std::fs::write(
        store
            .root()
            .join(&request.qa_run_id)
            .join("gate-receipt.json"),
        serde_json::to_vec(&legacy_v2_receipt).unwrap(),
    )
    .unwrap();
    assert_eq!(
        store.load_gate_receipt(&request.qa_run_id).unwrap(),
        legacy_v2_receipt.clone()
    );
    assert!(
        approved_gate_receipt(
            &request,
            &scenarios,
            &persisted.evidence,
            &result,
            reviewer_prompt_for_role(
                &request,
                &reviewer_run,
                &reviewer_session,
                PromptRole::RuntimeQaReviewerV2,
            ),
            result.completed_at_unix_ms,
        )
        .is_err()
    );
    std::fs::write(
        store.root().join(&request.qa_run_id).join("result.json"),
        serde_json::to_vec(&result).unwrap(),
    )
    .unwrap();
    assert!(store.load_gate_receipt(&request.qa_run_id).is_err());
    assert!(store.save_gate_receipt(&legacy_v2_receipt).is_err());

    let mut legacy = serde_json::to_value(&result).unwrap();
    legacy["schema_version"] = serde_json::json!(1);
    legacy
        .as_object_mut()
        .unwrap()
        .remove("reviewed_screenshot_ids");
    legacy.as_object_mut().unwrap().remove("review_coverage");
    std::fs::write(
        store.root().join(&request.qa_run_id).join("result.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();

    let loaded = store.load_run(&request.qa_run_id).unwrap().3;
    assert_eq!(loaded.schema_version, 1);
    assert!(loaded.reviewed_screenshot_ids.is_empty());
    assert!(loaded.review_coverage.is_empty());
    assert!(
        crate::runtime_qa::validate_current_report(
            &request,
            &scenarios,
            &persisted.evidence,
            &loaded,
        )
        .is_err()
    );
    assert_eq!(
        Some(store.load_gate_receipt(&request.qa_run_id).unwrap()),
        Some(legacy_v2_receipt)
    );
    assert!(
        approved_gate_receipt(
            &request,
            &scenarios,
            &persisted.evidence,
            &loaded,
            reviewer_prompt(&request, &reviewer_run, &reviewer_session),
            result.completed_at_unix_ms,
        )
        .is_err()
    );
    assert!(store.save_gate_receipt(receipt.as_ref().unwrap()).is_err());
}
