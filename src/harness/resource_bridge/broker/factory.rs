use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use super::ResourceBridge;

impl ResourceBridge {
    pub(crate) fn start_for_task_repository(
        task_repository: &Path,
        runtime_source: Option<&Path>,
        task_id: Option<&str>,
        progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
        cancel: Arc<AtomicBool>,
    ) -> anyhow::Result<Self> {
        Self::start_inner(
            task_repository,
            None,
            runtime_source,
            task_id,
            progress,
            cancel,
            &crate::persistence::state_root(),
            None,
            None,
            None,
            None,
            None,
        )
    }

    pub(crate) fn start_for_task_repository_target(
        task_repository: &Path,
        dependency_root: &Path,
        runtime_source: Option<&Path>,
        task_id: Option<&str>,
        progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
        cancel: Arc<AtomicBool>,
    ) -> anyhow::Result<Self> {
        Self::start_inner(
            task_repository,
            Some(dependency_root),
            runtime_source,
            task_id,
            progress,
            cancel,
            &crate::persistence::state_root(),
            None,
            None,
            None,
            None,
            None,
        )
    }
}
