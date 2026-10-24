use super::super::*;
use sha2::Sha256;
use std::{path::Path, time::Duration};

pub(super) fn approved_spec_hash() -> String {
    sha256(b"REQ-TASK-WEB-1: submitting a non-empty message displays Saved")
}

pub(super) fn review_subject_hash(source_sha: &str, diff_hash: &str, spec_hash: &str) -> String {
    let subject = serde_json::to_vec(&(source_sha, diff_hash, spec_hash)).unwrap();
    sha256(&subject)
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn scenario(request: &crate::runtime_qa::QaRunRequestV1) -> QaScenarioV1 {
    QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec![
            "The approved Python build, test, and start receipts pass for this task revision."
                .into(),
        ],
        steps: vec![
            step(
                QaDriverActionV1::BrowserOpen {
                    url: "http://qa-service/".into(),
                },
                "The task-built form is visible",
                true,
            ),
            step(
                QaDriverActionV1::BrowserFill {
                    locator: QaBrowserLocatorV1::Label {
                        label: "Message".into(),
                    },
                    value: "synthetic task QA input".into(),
                },
                "The synthetic message is in the form",
                false,
            ),
            step(
                QaDriverActionV1::BrowserPress {
                    locator: QaBrowserLocatorV1::Label {
                        label: "Message".into(),
                    },
                    key: "Enter".into(),
                },
                "The task-built application accepts the form submission",
                false,
            ),
            step(
                QaDriverActionV1::BrowserAssertText {
                    locator: QaBrowserLocatorV1::TestId {
                        test_id: "status".into(),
                    },
                    expected_text: "Saved".into(),
                },
                "The saved state is visible",
                true,
            ),
        ],
        expected_results: vec![
            "The approved startup artifact serves the task form.".into(),
            "Submitting a non-empty message displays Saved.".into(),
        ],
        negative_paths: Vec::new(),
        // The fixture reviewer below verifies functional evidence only. The
        // pixel-aware visual review contract is exercised by its own tests.
        visual_expectations: Vec::new(),
        redaction_regions: Vec::new(),
        priority: 1,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    docker: &bollard::Docker,
    lock: &ImageLock,
    browser_image: &crate::execution::docker::images::SelectedImage,
    service_image: &crate::execution::docker::images::SelectedImage,
    request: &crate::runtime_qa::QaRunRequestV1,
    scenario: &QaScenarioV1,
    recipe: &crate::project_runtime::ExecutionRecipeV1,
    artifact_upload: &QaContainerUpload,
    artifact_root: &Path,
    evidence_root: &Path,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(run_async(
        docker,
        lock,
        browser_image,
        service_image,
        request,
        scenario,
        recipe,
        artifact_upload,
        artifact_root,
        evidence_root,
    ));
}

#[allow(clippy::too_many_arguments)]
async fn run_async(
    docker: &bollard::Docker,
    lock: &ImageLock,
    browser_image: &crate::execution::docker::images::SelectedImage,
    service_image: &crate::execution::docker::images::SelectedImage,
    request: &crate::runtime_qa::QaRunRequestV1,
    scenario: &QaScenarioV1,
    recipe: &crate::project_runtime::ExecutionRecipeV1,
    artifact_upload: &QaContainerUpload,
    artifact_root: &Path,
    evidence_root: &Path,
) {
    request.validate().unwrap();
    scenario.validate(request).unwrap();
    let browser_request = build_browser_request(request, scenario).unwrap();
    let request_upload = QaContainerUpload::request_file(&browser_request, 256 * 1024).unwrap();
    let mut resources = resources();
    resources.process_count = 128;
    let (_cancel_sender, mut cancel) = tokio::sync::watch::channel(false);
    let preview = super::super::super::service::start_web_preview_with_lock(
        docker,
        QaWebPreviewSpec {
            request,
            browser_image,
            service_image,
            recipe,
            artifact_upload,
            dependency_caches: &[],
            preview_port: 4173,
            health_path: "/",
            resources: &resources,
        },
        &mut cancel,
        lock,
    )
    .await
    .unwrap();

    let mut driver_id = None;
    let result: Result<_, String> = async {
        if preview.service_lease.task_build_sha != request.build_artifact_sha256
            || preview.service_lease.startup_recipe_ref != recipe.recipe_id
            || preview.service_lease.startup_recipe_sha256 != request.startup_recipe_sha256
        {
            return Err("QA service lease does not match the approved task artifact".into());
        }
        validate_preview_relay_network(
            docker,
            request,
            &preview.service_lease,
            &preview.relay_binding,
        )
        .await
        .map_err(|error| error.message)?;
        let artifacts =
            [QaArtifactMount::inspect(artifact_root, "/workspace")
                .map_err(|error| error.message)?];
        let uploads = [artifact_upload.clone(), request_upload];
        let spec = QaDriverContainerSpec {
            owner_id: &request.execution_session_id,
            request,
            task_uid: &request.parent_task_uid,
            driver: "browser",
            image: browser_image,
            artifacts: &artifacts,
            dependency_caches: &[],
            network: QaDriverNetwork::InternalPreviewRelay {
                network_id: preview.relay_binding.relay_browser_network_id.clone(),
                relay_container_id: preview.relay_binding.relay_container_id.clone(),
            },
            service_lease: Some(&preview.service_lease),
            relay_binding: Some(&preview.relay_binding),
            resources: &resources,
        };
        let body = spec
            .create_body_with_lock(lock)
            .map_err(|error| error.message)?;
        let container_id = create_driver_container(
            docker,
            &format!("koolade-qa-{}", request.qa_run_id),
            body,
            &uploads,
            Duration::from_secs(120),
            16 * 1024 * 1024,
            &mut cancel,
        )
        .await
        .map_err(|error| error.message)?;
        driver_id = Some(container_id.clone());
        let mut binding = preview.relay_binding.clone();
        binding.browser_container_id = Some(container_id.clone());
        validate_browser_driver_network(docker, request, &binding, &container_id)
            .await
            .map_err(|error| error.message)?;
        let output = run_created_driver(
            docker,
            &container_id,
            Duration::from_secs(120),
            16 * 1024 * 1024,
            &mut cancel,
        )
        .await
        .map_err(|error| error.message)?;
        parse_browser_output(&output.stdout, request, scenario)
    }
    .await;
    let driver_cleanup = if let Some(container_id) = driver_id {
        cleanup_driver_container(docker, &container_id).await
    } else {
        Ok(())
    };
    let preview_cleanup =
        cleanup_web_preview(docker, &preview.service_lease, &preview.relay_binding).await;
    driver_cleanup.unwrap();
    preview_cleanup.unwrap();
    let output = result.unwrap();
    assert!(output.completed);
    assert_eq!(output.assertions.len(), 1);
    assert_eq!(
        output.assertions[0].status,
        crate::runtime_qa::QaAssertionStatus::Passed
    );
    assert!(
        output
            .screenshots
            .iter()
            .any(|screenshot| screenshot.width >= 320)
    );
    // The persistence reviewer is a deterministic fixture; visual judgment is
    // proven by the screenshot-inspection reviewer path covered separately.
    super::super::lifecycle::review_and_persist(
        request,
        scenario.clone(),
        &preview.service_lease,
        output,
        evidence_root.to_path_buf(),
    );
}
