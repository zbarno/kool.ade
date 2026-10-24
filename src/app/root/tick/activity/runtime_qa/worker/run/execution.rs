use std::sync::{Arc, atomic::AtomicBool};

use super::{super::super::request::PreparedRequest, run_reviewed};
use crate::{
    execution::{
        contract::ResourceBudgetV1,
        docker::{
            images::{ImageLock, RuntimeProfile, SelectedImage},
            qa_container::{QaArtifactMount, QaContainerUpload},
        },
        session::ExecutionSession,
    },
    runtime_qa::{
        QaPlatform, QaRunRequestV1, QaScenarioV1, QaTargetAuthorization, QaValidatedTargetV1,
        RuntimeQaStore,
    },
};

#[allow(clippy::too_many_arguments)]
pub(super) fn run_service(
    session: &ExecutionSession,
    prepared: &PreparedRequest,
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    artifact_upload: &QaContainerUpload,
    artifact_mount: QaArtifactMount,
    resources: &ResourceBudgetV1,
    browser_image: &SelectedImage,
    architecture: String,
    dependency_caches: &[crate::harness::resource_bridge::CacheSnapshot],
    store: &RuntimeQaStore,
    harness: Box<dyn crate::harness::AiHarness>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    let (port, health_path) = service_readiness(prepared)?;
    session.recover_qa_web_preview(request).map_err(|error| {
        format!(
            "stale task-scoped QA preview recovery failed: {}",
            error.message
        )
    })?;
    let image_lock = ImageLock::bundled().map_err(|error| error.message)?;
    let required_tools = std::iter::once(prepared.startup_recipe.executable_id.clone())
        .chain(
            prepared
                .startup_recipe
                .prerequisite_tool_ids
                .iter()
                .cloned(),
        )
        .collect::<Vec<_>>();
    let service_image = image_lock
        .select_with_tools(
            &[
                RuntimeProfile::Node,
                RuntimeProfile::Rust,
                RuntimeProfile::Python,
                RuntimeProfile::Go,
                RuntimeProfile::General,
                RuntimeProfile::BrowserQa,
            ],
            &architecture,
            &required_tools,
        )
        .map_err(|error| error.message)?;
    let preview = session
        .start_qa_web_preview(
            crate::execution::docker::qa_container::QaWebPreviewSpec {
                request,
                browser_image,
                service_image: &service_image,
                recipe: &prepared.startup_recipe,
                artifact_upload,
                dependency_caches,
                preview_port: port,
                health_path: &health_path,
                resources,
            },
            cancel,
        )
        .map_err(|error| format!("approved task preview could not start: {}", error.message))?;
    if let Err(error) = store.save_service_lease(request, &preview.service_lease) {
        let _ = session.cleanup_qa_web_preview(&preview.service_lease, &preview.relay_binding);
        return Err(format!(
            "task preview lease could not be persisted: {error}"
        ));
    }
    let driver = crate::runtime_qa::drivers::docker::DockerQaScenarioDriver {
        session,
        request,
        image: browser_image,
        artifact_mount,
        artifact_upload,
        dependency_caches: &[],
        resources,
        desktop_target: None,
        service_lease: Some(&preview.service_lease),
        relay_binding: Some(&preview.relay_binding),
        store,
        cancellation: cancel,
    };
    let authorization = match request.relevant_platforms.as_slice() {
        [QaPlatform::Web] => QaTargetAuthorization::Web(&preview.service_lease),
        [QaPlatform::HeadlessService] => {
            QaTargetAuthorization::HeadlessService(&preview.service_lease)
        }
        _ => return Err("task service preview does not match the approved QA platform".into()),
    };
    let result = run_reviewed(
        request,
        scenarios,
        authorization,
        driver,
        harness,
        &prepared.workspace,
        &prepared.task_clone,
        store,
        cancelled,
    );
    let cleanup = session
        .cleanup_qa_web_preview(&preview.service_lease, &preview.relay_binding)
        .map_err(|error| format!("QA task preview cleanup failed: {}", error.message));
    result.and(cleanup)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_desktop(
    session: &ExecutionSession,
    prepared: &PreparedRequest,
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    artifact_upload: &QaContainerUpload,
    artifact_mount: QaArtifactMount,
    resources: &ResourceBudgetV1,
    image: &SelectedImage,
    dependency_caches: &[crate::harness::resource_bridge::CacheSnapshot],
    store: &RuntimeQaStore,
    harness: Box<dyn crate::harness::AiHarness>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    let target = desktop_target(prepared, request)?;
    let driver = crate::runtime_qa::drivers::docker::DockerQaScenarioDriver {
        session,
        request,
        image,
        artifact_mount,
        artifact_upload,
        dependency_caches,
        resources,
        desktop_target: Some(&target),
        service_lease: None,
        relay_binding: None,
        store,
        cancellation: cancel,
    };
    run_reviewed(
        request,
        scenarios,
        QaTargetAuthorization::LinuxDesktop(&target),
        driver,
        harness,
        &prepared.workspace,
        &prepared.task_clone,
        store,
        cancelled,
    )
}

fn service_readiness(prepared: &PreparedRequest) -> Result<(u16, String), String> {
    service_readiness_for(
        prepared.target.app_surface,
        prepared.readiness_probe.as_ref(),
        &prepared.target.port_needs,
    )
}

pub(super) fn service_readiness_for(
    surface: crate::project_runtime::AppSurface,
    probe: Option<&crate::project_runtime::ReadinessProbeV1>,
    port_needs: &[u16],
) -> Result<(u16, String), String> {
    let probe = probe.ok_or_else(|| "service target has no approved readiness probe".to_owned())?;
    let (port, path) = match (surface, probe) {
        (
            crate::project_runtime::AppSurface::Web,
            crate::project_runtime::ReadinessProbeV1::HttpOnApprovedPort { port, path, .. },
        )
        | (
            crate::project_runtime::AppSurface::HeadlessService,
            crate::project_runtime::ReadinessProbeV1::LocalServiceHealth { port, path },
        ) => (*port, path.clone()),
        _ => {
            return Err(
                "service QA requires a readiness probe approved for the selected target surface"
                    .into(),
            );
        }
    };
    if !port_needs.contains(&port) {
        return Err(
            "service readiness port is not listed in the approved target capability".into(),
        );
    }
    Ok((port, path))
}

pub(super) fn desktop_target(
    prepared: &PreparedRequest,
    request: &QaRunRequestV1,
) -> Result<QaValidatedTargetV1, String> {
    let executable_id = match prepared.readiness_probe.as_ref() {
        Some(crate::project_runtime::ReadinessProbeV1::ProcessAlive { executable_id }) => {
            executable_id
        }
        _ => return Err("desktop QA requires an approved ProcessAlive readiness probe".into()),
    };
    if executable_id != &prepared.startup_recipe.executable_id {
        return Err(
            "desktop start recipe and ProcessAlive executable identity do not match".into(),
        );
    }
    let matching = prepared
        .artifact_paths
        .iter()
        .filter(|path| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                == Some(executable_id.as_str())
        })
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(
            "desktop start executable is not uniquely named in the receipt-approved artifact set"
                .into(),
        );
    }
    let executable = format!("/workspace/{}", matching[0]);
    let working_directory = if prepared.startup_recipe.working_directory_relative == "." {
        "/workspace".into()
    } else {
        format!(
            "/workspace/{}",
            prepared.startup_recipe.working_directory_relative
        )
    };
    let target = QaValidatedTargetV1 {
        task_uid: request.parent_task_uid.clone(),
        build_artifact_sha256: request.build_artifact_sha256.clone(),
        startup_recipe_ref: request.startup_recipe_ref.clone(),
        startup_recipe_sha256: request.startup_recipe_sha256.clone(),
        executable,
        working_directory,
        arguments: prepared.startup_recipe.argv.clone(),
    };
    target.validate_for(request)?;
    Ok(target)
}
