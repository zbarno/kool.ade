use crate::{
    app::session::Project,
    core::lifecycle::TaskRecord,
    project_runtime::{
        AppSurface, ProjectRuntimeStore, RuntimeOperation, RuntimeReceiptState, RuntimeReceiptV1,
    },
};

pub(in crate::app::root::tick::activity::runtime_qa) fn supports_qa_surface(
    surface: AppSurface,
) -> bool {
    matches!(
        surface,
        AppSurface::Web | AppSurface::LinuxDesktop | AppSurface::HeadlessService
    )
}

pub(in crate::app::root::tick::activity::runtime_qa) fn workspace_target_path(
    target_root: &str,
    path: &str,
) -> Result<String, String> {
    if target_root == "." || target_root == "./" {
        Ok(path.to_owned())
    } else if path == "." {
        Ok(target_root.to_owned())
    } else {
        Ok(format!("{target_root}/{path}"))
    }
}

pub(in crate::app::root::tick::activity::runtime_qa) fn selected_headless_target(
    project: &Project,
    task: &TaskRecord,
) -> bool {
    let Some(planning_uid) = task.planning_item_uid.as_deref() else {
        return false;
    };
    let Ok(Some(manifest)) =
        ProjectRuntimeStore::new(&project.state.planning_store).load_manifest()
    else {
        return false;
    };
    manifest.candidate_targets.iter().any(|target| {
        target.app_surface == AppSurface::HeadlessService
            && target.owning_task_id == planning_uid
            && manifest.is_active_target(planning_uid, &target.target_id)
    })
}

pub(in crate::app::root::tick::activity::runtime_qa) fn latest_successful_build_receipt<'a>(
    receipts: &'a [RuntimeReceiptV1],
    target_id: &str,
    source_sha: &str,
    recipe_id: &str,
) -> Option<&'a RuntimeReceiptV1> {
    receipts
        .iter()
        .filter(|receipt| {
            receipt.operation == RuntimeOperation::Build
                && receipt.state == RuntimeReceiptState::Passed
                && receipt.target_id == target_id
                && receipt.source_sha == source_sha
                && receipt.recipe_id.as_deref() == Some(recipe_id)
        })
        .max_by_key(|receipt| (receipt.observed_at_unix_ms, receipt.receipt_id.clone()))
}

pub(in crate::app::root::tick::activity::runtime_qa) fn latest_start_receipt<'a>(
    receipts: &'a [RuntimeReceiptV1],
    task_id: &str,
    target_id: &str,
    source_sha: &str,
    recipe_id: &str,
) -> Option<&'a RuntimeReceiptV1> {
    receipts
        .iter()
        .filter(|receipt| {
            receipt.operation == RuntimeOperation::Start
                && receipt.task_id == task_id
                && receipt.target_id == target_id
                && receipt.source_sha == source_sha
                && receipt.recipe_id.as_deref() == Some(recipe_id)
        })
        .max_by_key(|receipt| (receipt.observed_at_unix_ms, receipt.receipt_id.clone()))
}
