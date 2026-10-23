use super::report_fixture;
use crate::runtime_qa::*;

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
    let store_root = root.path().join("qa");
    std::fs::create_dir(&store_root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&store_root, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let store = RuntimeQaStore::new(&store_root);
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
    // Production boundary: persisted screenshots and reports are private even
    // when an existing store root or the process umask permits group access. A
    // serializer test cannot cover PlanningStore's image write path; this uses
    // a temporary filesystem only and has low setup cost.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode();
        let run_dir = store_root.join(&fixture.request.qa_run_id);
        let screenshot_path = store_root.join(&fixture.evidence.screenshots[0].storage_ref);
        assert_eq!(mode(&store_root) & 0o777, 0o700);
        assert_eq!(mode(&run_dir) & 0o777, 0o700);
        assert_eq!(mode(&run_dir.join("request.json")) & 0o777, 0o600);
        assert_eq!(mode(screenshot_path.parent().unwrap()) & 0o777, 0o700);
        assert_eq!(mode(&screenshot_path) & 0o777, 0o600);
    }
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

    // Existing schema-1/2 stores may have been created before private
    // permissions were enforced. Reading the report and its screenshots must
    // harden that existing tree before returning any content.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let run_dir = store_root.join(&fixture.request.qa_run_id);
        let screenshot_path = store_root.join(&fixture.evidence.screenshots[0].storage_ref);
        let screenshot_dir = screenshot_path.parent().unwrap();
        let evidence_run_dir = screenshot_dir.parent().unwrap();
        let evidence_root = evidence_run_dir.parent().unwrap();
        let permissive_dir = std::fs::Permissions::from_mode(0o755);
        for directory in [
            store_root.as_path(),
            run_dir.as_path(),
            evidence_root,
            evidence_run_dir,
            screenshot_dir,
        ] {
            std::fs::set_permissions(directory, permissive_dir.clone()).unwrap();
        }
        let permissive_file = std::fs::Permissions::from_mode(0o644);
        for name in [
            "request.json",
            "scenarios.json",
            "evidence.json",
            "result.json",
        ] {
            std::fs::set_permissions(run_dir.join(name), permissive_file.clone()).unwrap();
        }
        std::fs::set_permissions(&screenshot_path, permissive_file).unwrap();

        let loaded = store
            .load_run_with_screenshots(&fixture.request.qa_run_id)
            .unwrap();
        assert_eq!(loaded.screenshots.len(), 1);
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&store_root), 0o700);
        assert_eq!(mode(&run_dir), 0o700);
        assert_eq!(mode(&run_dir.join("request.json")), 0o600);
        assert_eq!(mode(screenshot_dir), 0o700);
        assert_eq!(mode(&screenshot_path), 0o600);
    }

    let scenario_path = store
        .root()
        .join(&fixture.request.qa_run_id)
        .join("scenarios.json");
    let mut changed_scenarios = fixture.scenarios.clone();
    changed_scenarios[0]
        .visual_expectations
        .push("Text remains readable after a resize".into());
    std::fs::write(
        &scenario_path,
        serde_json::to_vec(&changed_scenarios).unwrap(),
    )
    .unwrap();
    assert!(store.load_run(&fixture.request.qa_run_id).is_err());
    std::fs::write(
        &scenario_path,
        serde_json::to_vec(&fixture.scenarios).unwrap(),
    )
    .unwrap();

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
