use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod protocol;
mod resource;
mod tools;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BoundaryAccess {
    ReadOnly,
    Implementation,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServerConfigV2 {
    pub protocol_version: u16,
    pub access: BoundaryAccess,
    pub session_id: String,
    pub task_id: String,
    pub workspace_id: String,
    pub capability_id: String,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub target_arch: Option<String>,
    pub resource_socket: Option<PathBuf>,
    pub resource_cache: Option<PathBuf>,
}

pub(crate) type ServerConfig = ServerConfigV2;

impl ServerConfigV2 {
    pub(crate) fn implementation(
        binding: &super::session_bridge::SessionBinding,
        bridge: &crate::harness::resource_bridge::ResourceBridge,
    ) -> Self {
        Self {
            protocol_version: 2,
            access: BoundaryAccess::Implementation,
            session_id: binding.session_id.clone(),
            task_id: binding.task_id.clone(),
            workspace_id: binding.workspace_id.clone(),
            capability_id: binding.capability_id.clone(),
            target_id: Some(binding.target_id.clone()),
            target_arch: binding.target_arch.clone(),
            resource_socket: Some(bridge.socket_path().to_path_buf()),
            resource_cache: Some(bridge.cache_path().to_path_buf()),
        }
    }

    pub(crate) fn read_only(binding: &super::session_bridge::SessionBinding) -> Self {
        Self {
            protocol_version: 2,
            access: BoundaryAccess::ReadOnly,
            session_id: binding.session_id.clone(),
            task_id: binding.task_id.clone(),
            workspace_id: binding.workspace_id.clone(),
            capability_id: binding.capability_id.clone(),
            target_id: Some(binding.target_id.clone()),
            target_arch: binding.target_arch.clone(),
            resource_socket: None,
            resource_cache: None,
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.protocol_version == 2,
            "Unsupported MCP execution protocol"
        );
        crate::execution::contract::validate_identifier(&self.session_id)?;
        crate::execution::contract::validate_identifier(&self.task_id)?;
        crate::execution::contract::validate_identifier(&self.capability_id)?;
        let mut workspace_parts = self.workspace_id.split(':');
        let project_id = workspace_parts.next().unwrap_or_default();
        let task_id = workspace_parts.next().unwrap_or_default();
        let source_sha = workspace_parts.next().unwrap_or_default();
        anyhow::ensure!(
            workspace_parts.next().is_none()
                && crate::execution::contract::validate_identifier(project_id).is_ok()
                && crate::execution::contract::validate_identifier(task_id).is_ok()
                && crate::execution::contract::is_valid_sha(source_sha),
            "MCP workspace identity is invalid"
        );
        match self.access {
            BoundaryAccess::ReadOnly => {
                anyhow::ensure!(
                    self.resource_socket.is_none() && self.resource_cache.is_none(),
                    "Read-only MCP tools cannot access resource brokers"
                );
            }
            BoundaryAccess::Implementation => {
                anyhow::ensure!(
                    self.resource_socket.is_some() && self.resource_cache.is_some(),
                    "Implementation MCP tools require the application resource broker"
                );
                anyhow::ensure!(
                    self.target_id.is_some() && self.target_arch.is_some(),
                    "Implementation MCP cache target is unavailable"
                );
            }
        }
        Ok(())
    }
}

pub(crate) fn serve(config_path: &Path) -> anyhow::Result<()> {
    let metadata = std::fs::metadata(config_path)?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= 1024 * 1024,
        "MCP server configuration is invalid"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        anyhow::ensure!(
            metadata.uid() == unsafe { geteuid() } && metadata.permissions().mode() & 0o077 == 0,
            "MCP server configuration must be private to the current user"
        );
    }
    let config: ServerConfig = serde_json::from_slice(&std::fs::read(config_path)?)?;
    config.validate()?;
    protocol::serve(config_path, &config)
}

#[cfg(unix)]
unsafe extern "C" {
    fn geteuid() -> u32;
}
