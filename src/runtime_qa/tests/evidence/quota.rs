use super::*;

#[test]
// Production boundary: the persisted report path must reject individually
// valid screenshots whose aggregate exceeds the request quota before writing
// report records or image bytes. A validator-only test cannot prove the store
// enforces this before persistence. Uses a temporary filesystem; no external
// service, with low setup cost.
fn screenshot_set_over_aggregate_quota_is_rejected_before_persistence() {
    let mut fixture = report_fixture();
    let second_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut second = fixture.evidence.screenshots[0].clone();
    second.id = second_id.clone();
    second.storage_ref = format!(
        "qa-evidence/{}/screenshots/{second_id}.png",
        fixture.request.qa_run_id
    );
    fixture.evidence.screenshots.push(second);
    fixture.evidence.actions[1]
        .evidence_ids
        .push(second_id.clone());
    fixture.evidence.assertions[0]
        .evidence_refs
        .push(second_id.clone());
    fixture
        .result
        .reviewed_screenshot_ids
        .push(second_id.clone());
    fixture.result.review_coverage[0]
        .evidence_refs
        .push(second_id.clone());
    fixture.result.cases[0]
        .evidence_refs
        .push(second_id.clone());
    fixture.result.coverage.requirements[0]
        .evidence_refs
        .push(second_id.clone());
    fixture.request.bounded_limits.max_evidence_bytes = fixture.screenshot.bytes.len() as u64;
    fixture.result.evidence_manifest_sha256 = fixture.evidence.sha256().unwrap();

    let screenshots = vec![
        QaScreenshotArtifactV1 {
            id: fixture.screenshot.id.clone(),
            bytes: fixture.screenshot.bytes.clone(),
        },
        QaScreenshotArtifactV1 {
            id: second_id,
            bytes: fixture.screenshot.bytes.clone(),
        },
    ];
    let root = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(root.path().join("qa"));
    let error = store
        .save_run_with_screenshots(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result,
            &screenshots,
        )
        .unwrap_err();

    assert_eq!(
        error,
        "runtime QA screenshot set exceeds its aggregate evidence quota"
    );
    assert!(
        !store
            .root()
            .join(&fixture.request.qa_run_id)
            .join("request.json")
            .exists()
    );
}
