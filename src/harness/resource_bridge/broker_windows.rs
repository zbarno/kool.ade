#![allow(dead_code)]

use crate::harness::{DependencyNeed, DependencyRequest};
use std::path::Path;

pub(crate) struct ResourceBridge;

impl ResourceBridge {
    pub(crate) fn start_for_task_repository(
        _task_repository: &Path,
        _runtime_source: Option<&Path>,
        _task_id: Option<&str>,
        _progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
        _cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<Self> {
        anyhow::bail!("Docker resource brokering is unavailable on Windows")
    }

    pub(crate) fn start_for_task_repository_target(
        _task_repository: &Path,
        _dependency_root: &Path,
        _runtime_source: Option<&Path>,
        _task_id: Option<&str>,
        _progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
        _cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<Self> {
        anyhow::bail!("Docker resource brokering is unavailable on Windows")
    }

    pub(crate) fn bind_execution_session(
        &self,
        _session: std::sync::Arc<crate::execution::session::ExecutionSession>,
    ) -> anyhow::Result<()> {
        anyhow::bail!("Docker resource brokering is unavailable on Windows")
    }

    pub(crate) fn request_system_tools(
        &self,
        _need: DependencyNeed,
    ) -> Result<DependencyRequest, String> {
        Err("Docker resource brokering is unavailable on Windows".into())
    }

    pub(crate) fn request_locked_dependency(
        &self,
        _need: DependencyNeed,
    ) -> Result<DependencyRequest, String> {
        Err("Docker resource brokering is unavailable on Windows".into())
    }

    pub(crate) fn cache_snapshot(
        &self,
        _task_id: &str,
        _ecosystem: crate::execution::contract::DependencyCacheEcosystemV1,
        _project_id: &str,
        _resolved_integrity: &str,
        _target_arch: &str,
        _target_id: &str,
    ) -> anyhow::Result<super::cache_snapshot::CacheSnapshot> {
        anyhow::bail!("Docker resource brokering is unavailable on Windows")
    }

    pub(crate) fn validate_dependency_cache_ref(
        &self,
        _dependency_request_id: &str,
        _cache_ref: &crate::execution::contract::DependencyCacheRefV1,
        _project_id: &str,
        _target_id: &str,
        _target_arch: &str,
    ) -> anyhow::Result<()> {
        anyhow::bail!("Docker resource brokering is unavailable on Windows")
    }

    pub(crate) fn socket_path(&self) -> &Path {
        Path::new("")
    }

    pub(crate) fn cache_path(&self) -> &Path {
        Path::new("")
    }

    pub(crate) fn attention_detail(&self) -> Option<String> {
        None
    }

    pub(crate) fn dependency_request(&self) -> Option<DependencyRequest> {
        None
    }
}
