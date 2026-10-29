use crate::{
    app::session::Project,
    core::lifecycle::TaskRecord,
    execution::docker::qa_container::{QaArtifactMount, QaContainerUpload},
    runtime_qa::{QaPlatform, QaRunRequestV1, QaValidatedTargetV1},
};

use super::{execution, prepare_dispatch_artifacts};

pub(crate) struct RootlessDesktopDispatch {
    pub request: QaRunRequestV1,
    pub target: QaValidatedTargetV1,
    pub artifact_upload: QaContainerUpload,
    pub artifact_mount: QaArtifactMount,
}

pub(crate) struct RootlessTaskQaDispatch {
    pub request: QaRunRequestV1,
    pub startup_recipe: crate::project_runtime::ExecutionRecipeV1,
    pub target: Option<QaValidatedTargetV1>,
    pub artifact_upload: QaContainerUpload,
    pub artifact_mount: QaArtifactMount,
    pub task_clone: std::path::PathBuf,
}

pub(crate) fn prepare_rootless_task_qa_dispatch(
    project: &Project,
    task: &TaskRecord,
) -> Result<RootlessTaskQaDispatch, String> {
    let prepared = super::super::super::request::prepare(project, task)?;
    let (request, artifact_upload) = prepare_dispatch_artifacts(&prepared)?;
    let target = if request.relevant_platforms == [QaPlatform::LinuxDesktop] {
        Some(execution::desktop_target(&prepared, &request)?)
    } else {
        None
    };
    let artifact_mount = QaArtifactMount::inspect(&prepared.task_clone, "/workspace")
        .map_err(|error| error.message)?;
    Ok(RootlessTaskQaDispatch {
        request,
        startup_recipe: prepared.startup_recipe,
        target,
        artifact_upload,
        artifact_mount,
        task_clone: prepared.task_clone,
    })
}

pub(crate) fn prepare_rootless_desktop_dispatch(
    project: &Project,
    task: &TaskRecord,
) -> Result<RootlessDesktopDispatch, String> {
    let dispatch = prepare_rootless_task_qa_dispatch(project, task)?;
    let target = dispatch
        .target
        .ok_or_else(|| "the selected runtime target is not a Linux desktop target".to_owned())?;
    Ok(RootlessDesktopDispatch {
        request: dispatch.request,
        target,
        artifact_upload: dispatch.artifact_upload,
        artifact_mount: dispatch.artifact_mount,
    })
}
