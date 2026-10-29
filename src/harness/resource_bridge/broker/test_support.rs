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
        None,
        None,
    )
}

impl ResourceBridge {
    pub(crate) fn start_with_npm_fixture_archive(
        worktree: &Path,
        task_id: &str,
        progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
        cancel: Arc<std::sync::atomic::AtomicBool>,
        state_root: &Path,
        fixture: super::super::NpmFixtureArchive,
    ) -> anyhow::Result<Self> {
        let npm_operations = super::super::npm::test_fixture_preparation_operations(
            fixture.archive,
            fixture.archive_url,
            fixture.registry_url,
        );
        Self::start_inner(
            worktree,
            None,
            None,
            Some(task_id),
            progress,
            cancel,
            state_root,
            Some(npm_operations),
            None,
            None,
            None,
            None,
        )
    }
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
        None,
        None,
    )
}

pub(in crate::harness::resource_bridge) fn start_with_state_root_and_pip_operations(
    worktree: &Path,
    dependency_root: Option<&Path>,
    task_id: Option<&str>,
    progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    state_root: &Path,
    pip_operations: Arc<super::super::pip::SharedPreparationOperations>,
) -> anyhow::Result<ResourceBridge> {
    ResourceBridge::start_inner(
        worktree,
        dependency_root,
        None,
        task_id,
        progress,
        cancel,
        state_root,
        None,
        None,
        Some(pip_operations),
        None,
        None,
    )
}

impl ResourceBridge {
    pub(crate) fn start_with_state_root_and_go_operations(
        worktree: &Path,
        dependency_root: &Path,
        task_id: &str,
        progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
        cancel: Arc<std::sync::atomic::AtomicBool>,
        state_root: &Path,
        go_operations: Arc<super::super::go::SharedPreparationOperations>,
    ) -> anyhow::Result<Self> {
        Self::start_inner(
            worktree,
            Some(dependency_root),
            None,
            Some(task_id),
            progress,
            cancel,
            state_root,
            None,
            None,
            None,
            Some(go_operations),
            None,
        )
    }
}
