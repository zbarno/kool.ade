use super::*;

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
    let (request, evidence, gallery) = store
        .load_screenshot_gallery(&fixture.request.qa_run_id, 0, 12)
        .unwrap();
    assert_eq!(request, fixture.request);
    assert_eq!(evidence, fixture.evidence);
    assert_eq!(gallery.len(), 1);
    assert_eq!(gallery[0].id, fixture.screenshot.id);
    assert_eq!(gallery[0].bytes, fixture.screenshot.bytes);
    assert!(
        store
            .load_screenshot_gallery(&fixture.request.qa_run_id, 0, 0)
            .is_err()
    );

    let artifact_path = store
        .root()
        .join(&fixture.evidence.screenshots[0].storage_ref);
    std::fs::write(artifact_path, b"tampered PNG").unwrap();
    assert!(store.load_run(&fixture.request.qa_run_id).is_err());
    assert!(
        store
            .load_screenshot_gallery(&fixture.request.qa_run_id, 0, 12)
            .is_err()
    );
}

#[test]
// Covers the persisted QA store boundary: a valid first page stays available
// when an unrelated screenshot is corrupt, while that screenshot is rejected
// when requested. A pure validator test cannot prove page-scoped filesystem
// reads; the test uses a temporary local store and has no external dependency.
fn screenshot_gallery_reads_and_validates_only_the_requested_page() {
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
    fixture.result.evidence_manifest_sha256 = fixture.evidence.sha256().unwrap();
    let second_artifact = QaScreenshotArtifactV1 {
        id: second_id,
        bytes: fixture.screenshot.bytes.clone(),
    };
    let screenshots = vec![fixture.screenshot, second_artifact];

    let root = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(root.path().join("qa"));
    store
        .save_run_with_screenshots(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result,
            &screenshots,
        )
        .unwrap();
    std::fs::write(
        store
            .root()
            .join(&fixture.evidence.screenshots[1].storage_ref),
        b"tampered PNG",
    )
    .unwrap();

    let (request, evidence, first_page) = store
        .load_screenshot_gallery(&fixture.request.qa_run_id, 0, 1)
        .unwrap();
    assert_eq!(request, fixture.request);
    assert_eq!(evidence.screenshots.len(), 2);
    assert_eq!(first_page.len(), 1);
    assert_eq!(first_page[0].id, fixture.evidence.screenshots[0].id);
    assert!(
        store
            .load_screenshot_gallery(&fixture.request.qa_run_id, 1, 1)
            .is_err()
    );
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
