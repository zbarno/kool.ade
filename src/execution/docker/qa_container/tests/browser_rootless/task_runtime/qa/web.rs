use super::*;
use std::time::Duration;

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
    readiness_path: &str,
    expected_browser_outcome: super::ExpectedBrowserOutcome,
) {
    let dependency_caches = super::dependency_caches_for_image(request, service_image);
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
        &dependency_caches,
        artifact_root,
        evidence_root,
        readiness_path,
        expected_browser_outcome,
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
    dependency_caches: &[crate::harness::resource_bridge::CacheSnapshot],
    artifact_root: &Path,
    evidence_root: &Path,
    readiness_path: &str,
    expected_browser_outcome: super::ExpectedBrowserOutcome,
) {
    request.validate().unwrap();
    scenario.validate(request).unwrap();
    let browser_request = build_browser_request(request, scenario).unwrap();
    let request_upload = QaContainerUpload::request_file(&browser_request, 256 * 1024).unwrap();
    let mut resources = resources();
    resources.process_count = 128;
    let (_cancel_sender, mut cancel) = tokio::sync::watch::channel(false);
    let preview = super::super::super::super::service::start_web_preview_with_lock(
        docker,
        QaWebPreviewSpec {
            request,
            browser_image,
            service_image,
            recipe,
            artifact_upload,
            dependency_caches,
            preview_port: 4173,
            health_path: readiness_path,
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
            // Dependency caches are staged into the approved app service;
            // the browser driver consumes only the preview relay.
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
        assert_runtime_user_cannot_change_workspace_permissions(
            docker,
            &container_id,
            &browser_image.runtime_user,
        )
        .await?;
        let mut binding = preview.relay_binding.clone();
        binding.browser_container_id = Some(container_id.clone());
        validate_browser_driver_network(docker, request, &binding, &container_id)
            .await
            .map_err(|error| error.message)?;
        let output = run_created_driver_with_status(
            docker,
            &container_id,
            Duration::from_secs(120),
            16 * 1024 * 1024,
            &mut cancel,
        )
        .await
        .map_err(|error| error.message)?;
        match expected_browser_outcome {
            super::ExpectedBrowserOutcome::Pass if output.exit_code != 0 => {
                return Err(format!(
                    "passing browser QA driver exited with status {}",
                    output.exit_code
                ));
            }
            super::ExpectedBrowserOutcome::Fail if output.exit_code == 0 => {
                return Err("failing browser assertion exited with a successful status".into());
            }
            _ => {}
        }
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
    assert!(
        output
            .screenshots
            .iter()
            .any(|screenshot| screenshot.width >= 320)
    );
    let expected_outcome = match expected_browser_outcome {
        super::ExpectedBrowserOutcome::Pass => {
            assert_eq!(
                output.assertions[0].status,
                crate::runtime_qa::QaAssertionStatus::Passed
            );
            crate::runtime_qa::QaOutcome::Pass
        }
        super::ExpectedBrowserOutcome::Fail => {
            assert_eq!(
                output.assertions[0].status,
                crate::runtime_qa::QaAssertionStatus::Failed
            );
            crate::runtime_qa::QaOutcome::Fail
        }
    };
    // The persistence reviewer is a deterministic fixture; visual judgment is
    // proven by the screenshot-inspection reviewer path covered separately.
    super::super::super::lifecycle::review_expected_outcome_and_persist(
        request,
        scenario.clone(),
        &preview.service_lease,
        output,
        evidence_root.to_path_buf(),
        expected_outcome,
    );
}

async fn assert_runtime_user_cannot_change_workspace_permissions(
    docker: &bollard::Docker,
    container_id: &str,
    runtime_user: &str,
) -> Result<(), String> {
    // Only the rootless container can prove the executor UID mapping and
    // ownership fence; body/permission unit tests cannot prove chmod is denied.
    use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
    use futures_util::TryStreamExt;

    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let exec = docker
            .create_exec(
                container_id,
                CreateExecOptions::<String> {
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    cmd: Some(vec!["chmod".into(), "u+w".into(), "/workspace".into()]),
                    user: Some(runtime_user.into()),
                    privileged: Some(false),
                    ..Default::default()
                },
            )
            .await
            .map_err(|_| "QA workspace permission check could not be created")?;
        let StartExecResults::Attached { mut output, .. } = docker
            .start_exec(
                &exec.id,
                Some(StartExecOptions {
                    detach: false,
                    tty: false,
                    output_capacity: Some(1024),
                }),
            )
            .await
            .map_err(|_| "QA workspace permission check could not start")?
        else {
            return Err("QA workspace permission check did not attach");
        };
        while output
            .try_next()
            .await
            .map_err(|_| "QA workspace permission check output could not be read")?
            .is_some()
        {}
        let status = docker
            .inspect_exec(&exec.id)
            .await
            .map_err(|_| "QA workspace permission check result could not be read")?;
        Ok::<_, &'static str>(status.exit_code)
    })
    .await
    .map_err(|_| "QA workspace permission check exceeded its time bound")??;
    match result {
        Some(0) => Err("QA runtime user unexpectedly changed workspace permissions".into()),
        Some(_) => Ok(()),
        None => Err("QA workspace permission check ended without an exit status".into()),
    }
}
