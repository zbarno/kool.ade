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
        None,
        runtime_source,
        task_id,
        progress,
        cancel,
        state_root,
        Some(npm_operations),
        None,
        None,
    )
}

pub(in crate::harness::resource_bridge) fn start_with_state_root_and_cargo_operations_for_target(
    worktree: &Path,
    dependency_root: &Path,
    task_id: Option<&str>,
    progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    state_root: &Path,
    cargo_operations: Arc<super::super::cargo::SharedPreparationOperations>,
) -> anyhow::Result<ResourceBridge> {
    ResourceBridge::start_inner(
        worktree,
        Some(dependency_root),
        None,
        task_id,
        progress,
        cancel,
        state_root,
        None,
        Some(cargo_operations),
        None,
    )
}
