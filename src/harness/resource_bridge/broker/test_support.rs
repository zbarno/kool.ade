use super::ResourceBridge;
use std::{path::Path, sync::Arc};

pub(in crate::harness::resource_bridge) fn start_with_state_root_and_npm_operations(
    worktree: &Path,
    runtime_source: Option<&Path>,
    task_id: Option<&str>,
    progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    state_root: &Path,
    npm_operations: Arc<super::super::npm::SharedPreparationOperations>,
) -> anyhow::Result<ResourceBridge> {
    ResourceBridge::start_inner(
        worktree,
        runtime_source,
        task_id,
        progress,
        cancel,
        state_root,
        Some(npm_operations),
        None,
    )
}
