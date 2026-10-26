#![allow(dead_code)]

use std::path::Path;

#[derive(Clone, Debug)]
pub(crate) struct SessionBinding {
    pub session_id: String,
    pub capability_id: String,
    pub task_id: String,
    pub workspace_id: String,
    pub access: String,
    pub target_id: String,
    pub target_arch: Option<String>,
}

pub(crate) struct DependencyRelayStartResponse {
    pub(crate) request: crate::execution::remote::dependency::DependencyRelayRequestV1,
    pub(crate) broker_response: serde_json::Value,
}

pub(super) struct ToolResponse {
    pub(super) stdout: String,
    pub(super) stderr: String,
    pub(super) exit_code: Option<i32>,
    pub(super) is_error: bool,
}

pub(super) struct SessionBridge {
    pub binding: SessionBinding,
}

impl SessionBridge {
    pub fn start(
        _scratch: &Path,
        _session: std::sync::Arc<crate::execution::session::ExecutionSession>,
        _task_id: String,
        _access: String,
        _cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        _resource_bridge: Option<std::sync::Arc<crate::harness::resource_bridge::ResourceBridge>>,
    ) -> anyhow::Result<Self> {
        anyhow::bail!("Docker execution sessions are unavailable on Windows")
    }
}

pub(super) fn call(
    _config_path: &Path,
    _binding: &SessionBinding,
    _command: &str,
    _timeout_seconds: u64,
) -> anyhow::Result<ToolResponse> {
    anyhow::bail!("Docker execution sessions are unavailable on Windows")
}

pub(crate) fn stage_cache(
    _config_path: &Path,
    _binding: &SessionBinding,
    _dependency_request_id: &str,
    _snapshot: crate::harness::resource_bridge::CacheSnapshot,
) -> anyhow::Result<()> {
    anyhow::bail!("Docker execution sessions are unavailable on Windows")
}

pub(crate) fn request_dependency(
    _config_path: &Path,
    _binding: &SessionBinding,
    _need: crate::harness::DependencyNeed,
) -> anyhow::Result<DependencyRelayStartResponse> {
    anyhow::bail!("Docker execution sessions are unavailable on Windows")
}
