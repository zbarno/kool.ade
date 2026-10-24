use super::*;
use std::time::Duration;

#[path = "concurrency/assertions.rs"]
mod assertions;
#[path = "concurrency/driver.rs"]
mod driver;

/// Production boundary: two approved QA run IDs drive browser actions against
/// separate rootless task previews. The test confirms both delayed preview
/// requests are active at once, then verifies that cleaning one run leaves the
/// other run's containers and networks intact. A unit test cannot prove Docker
/// resource ownership or simultaneous in-flight requests. External dependency:
/// rootless Docker and the pinned browser/Python images. Setup cost: one extra
/// preview and cache plus two browser drivers with ten-second delayed requests.
pub(super) struct ParallelRunContext<'a> {
    pub docker: &'a bollard::Docker,
    pub request: &'a crate::runtime_qa::QaRunRequestV1,
    pub scenario: &'a QaScenarioV1,
    pub browser_image: &'a crate::execution::docker::images::SelectedImage,
    pub service_image: &'a crate::execution::docker::images::SelectedImage,
    pub lock: &'a ImageLock,
    pub artifact_root: &'a tempfile::TempDir,
    pub artifact_upload: &'a QaContainerUpload,
    pub recipe: &'a crate::project_runtime::ExecutionRecipeV1,
    pub preview: &'a super::super::super::QaWebPreview,
    pub resources: &'a ResourceBudgetV1,
}

pub(super) async fn assert_parallel_browser_runs_are_isolated(
    context: ParallelRunContext<'_>,
) -> Result<(), String> {
    let mut second_request = context.request.clone();
    second_request.qa_run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    second_request.execution_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    second_request.parent_task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    second_request.author_run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    second_request.author_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let cache_root = tempfile::tempdir().map_err(|error| error.to_string())?;
    std::fs::write(
        cache_root.path().join("package.tgz"),
        b"approved offline browser cache fixture",
    )
    .map_err(|error| error.to_string())?;
    let second_cache = crate::harness::resource_bridge::CacheSnapshot::capture(
        cache_root.path(),
        second_request
            .planning_item_uid
            .as_deref()
            .ok_or("parallel preview has no planning identity")?,
        crate::execution::contract::DependencyCacheEcosystemV1::Npm,
        &second_request.parent_task_uid,
        "sha256:synthetic-lockfile",
        "amd64",
        "local",
    )
    .map_err(|error| error.to_string())?;
    second_request.dependency_cache_refs = vec![crate::runtime_qa::QaDependencyCacheRefV1 {
        dependency_request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        cache_ref: second_cache.cache_ref.clone(),
    }];
    second_request.validate()?;

    let first_scenario = parallel_scenario(context.request, context.scenario)?;
    let second_scenario = parallel_scenario(&second_request, context.scenario)?;
    let (_preview_cancel_sender, mut cancel) = tokio::sync::watch::channel(false);
    let preview_spec = QaWebPreviewSpec {
        request: &second_request,
        browser_image: context.browser_image,
        service_image: context.service_image,
        recipe: context.recipe,
        artifact_upload: context.artifact_upload,
        dependency_caches: std::slice::from_ref(&second_cache),
        preview_port: 4173,
        health_path: "/",
        resources: context.resources,
    };
    let second_preview = super::super::service::start_web_preview_with_lock(
        context.docker,
        preview_spec,
        &mut cancel,
        context.lock,
    )
    .await
    .map_err(|error| error.message)?;
    if let Err(error) = validate_preview_relay_network(
        context.docker,
        &second_request,
        &second_preview.service_lease,
        &second_preview.relay_binding,
    )
    .await
    {
        let cleanup = cleanup_and_verify_preview(context.docker, &second_preview).await;
        return Err(match cleanup {
            Ok(()) => error.message,
            Err(cleanup) => format!(
                "secondary preview validation failed: {}; cleanup failed: {cleanup}",
                error.message
            ),
        });
    }

    let first_run = driver::run_browser_driver(
        context.docker,
        context.request,
        &first_scenario,
        context.browser_image,
        context.lock,
        context.artifact_root.path(),
        context.artifact_upload,
        context.preview,
        context.resources,
    );
    let second_run = driver::run_browser_driver(
        context.docker,
        &second_request,
        &second_scenario,
        context.browser_image,
        context.lock,
        context.artifact_root.path(),
        context.artifact_upload,
        &second_preview,
        context.resources,
    );
    let all_runs = async { tokio::join!(first_run, second_run) };
    let active_requests = assertions::wait_for_simultaneous_parallel_requests(
        context.docker,
        &context.preview.service_lease.task_container_id,
        &second_preview.service_lease.task_container_id,
        Duration::from_secs(20),
    );
    let ((first_result, second_result), active_request_result) =
        tokio::join!(all_runs, active_requests);

    let first_driver_cleanup = driver::cleanup_outcome(context.docker, &first_result).await;
    let second_driver_cleanup = driver::cleanup_outcome(context.docker, &second_result).await;
    let second_preview_cleanup = cleanup_and_verify_preview(context.docker, &second_preview).await;
    let first_preview_presence =
        assertions::assert_run_resources_present(context.docker, context.preview).await;

    let mut failures = Vec::new();
    if let Err(error) = active_request_result {
        failures.push(error);
    }
    if let Err(error) = first_driver_cleanup {
        failures.push(format!("first browser driver: {error}"));
    }
    if let Err(error) = second_driver_cleanup {
        failures.push(format!("second browser driver: {error}"));
    }
    if let Err(error) = second_preview_cleanup {
        failures.push(format!("second preview: {error}"));
    }
    if let Err(error) = first_preview_presence {
        failures.push(format!("first preview: {error}"));
    }

    let first_outcome = match first_result {
        Ok(outcome) => Some(outcome),
        Err(error) => {
            failures.push(format!("first browser run could not start: {error}"));
            None
        }
    };
    let second_outcome = match second_result {
        Ok(outcome) => Some(outcome),
        Err(error) => {
            failures.push(format!("second browser run could not start: {error}"));
            None
        }
    };
    if !failures.is_empty() {
        return Err(failures.join("; "));
    }
    let first_outcome = first_outcome.expect("successful first browser run was retained");
    let second_outcome = second_outcome.expect("successful second browser run was retained");
    let first_output = first_outcome.output?;
    let second_output = second_outcome.output?;

    if first_outcome.driver_container_id == second_outcome.driver_container_id
        || context.request.qa_run_id == second_request.qa_run_id
        || context.request.execution_session_id == second_request.execution_session_id
        || context.request.parent_task_uid == second_request.parent_task_uid
        || context.preview.service_lease.task_container_id
            == second_preview.service_lease.task_container_id
        || context.preview.service_lease.task_network_id
            == second_preview.service_lease.task_network_id
        || context.preview.relay_binding.relay_container_id
            == second_preview.relay_binding.relay_container_id
        || context.preview.relay_binding.relay_browser_network_id
            == second_preview.relay_binding.relay_browser_network_id
    {
        return Err("concurrent browser runs reused a run-owned resource identity".into());
    }
    for output in [&first_output, &second_output] {
        if !output.completed
            || output.assertions.is_empty()
            || output
                .assertions
                .iter()
                .any(|assertion| assertion.status != crate::runtime_qa::QaAssertionStatus::Passed)
        {
            return Err("a concurrent browser run did not complete its approved scenario".into());
        }
    }
    Ok(())
}

async fn cleanup_and_verify_preview(
    docker: &bollard::Docker,
    preview: &super::super::super::QaWebPreview,
) -> Result<(), String> {
    let cleanup = cleanup_web_preview(docker, &preview.service_lease, &preview.relay_binding)
        .await
        .map_err(|error| error.message);
    let retry = if cleanup.is_err() {
        Some(
            cleanup_web_preview(docker, &preview.service_lease, &preview.relay_binding)
                .await
                .map_err(|error| error.message),
        )
    } else {
        None
    };
    let verification = assertions::assert_run_resources_removed(docker, preview).await;
    let mut failures = Vec::new();
    if let Err(error) = cleanup {
        failures.push(format!("preview cleanup failed: {error}"));
    }
    if let Some(Err(error)) = retry {
        failures.push(format!("preview cleanup retry failed: {error}"));
    }
    if let Err(error) = verification {
        failures.push(error);
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn parallel_scenario(
    request: &crate::runtime_qa::QaRunRequestV1,
    source: &QaScenarioV1,
) -> Result<QaScenarioV1, String> {
    let mut scenario = source.clone();
    scenario.id = uuid::Uuid::new_v4().hyphenated().to_string();
    scenario.visual_expectations.clear();
    scenario.steps.insert(
        2,
        step(
            QaDriverActionV1::BrowserOpen {
                url: "http://qa-service/?defect=parallel".into(),
            },
            "Preview fixture is visible",
            false,
        ),
    );
    scenario.validate(request)?;
    Ok(scenario)
}
