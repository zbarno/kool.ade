use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod protocol;
mod resource;
mod sandbox;
mod tools;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BoundaryAccess {
    ReadOnly,
    Implementation,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServerConfig {
    pub access: BoundaryAccess,
    pub bwrap: PathBuf,
    pub root: PathBuf,
    pub args: Vec<String>,
    pub resource_socket: Option<PathBuf>,
    pub resource_cache: Option<PathBuf>,
}

impl ServerConfig {
    pub(crate) fn implementation(
        sandbox: &crate::harness::pi_sandbox::Sandbox,
        bridge: &crate::harness::resource_bridge::ResourceBridge,
    ) -> Self {
        Self {
            access: BoundaryAccess::Implementation,
            bwrap: sandbox.bwrap.clone(),
            root: sandbox.root.clone(),
            args: sandbox.args.clone(),
            resource_socket: Some(bridge.socket_path().to_path_buf()),
            resource_cache: Some(bridge.cache_path().to_path_buf()),
        }
    }

    pub(crate) fn read_only(sandbox: &crate::harness::pi_sandbox::PlanningSandbox) -> Self {
        Self {
            access: BoundaryAccess::ReadOnly,
            bwrap: sandbox.bwrap.clone(),
            root: sandbox.root.clone(),
            args: sandbox.args.clone(),
            resource_socket: None,
            resource_cache: None,
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.root.is_absolute() && self.root.is_dir(),
            "MCP workspace root is unavailable"
        );
        anyhow::ensure!(
            self.bwrap.is_absolute() && self.bwrap.is_file(),
            "Bubblewrap executable is unavailable"
        );
        for flag in ["--unshare-user", "--unshare-net", "--clearenv"] {
            anyhow::ensure!(
                self.args.iter().any(|arg| arg == flag),
                "MCP sandbox is missing required isolation flag {flag}"
            );
        }
        match self.access {
            BoundaryAccess::ReadOnly => {
                anyhow::ensure!(
                    self.resource_socket.is_none() && self.resource_cache.is_none(),
                    "Read-only MCP tools cannot access resource brokers"
                );
                anyhow::ensure!(
                    self.args.windows(3).any(|mount| {
                        mount[0] == "--ro-bind"
                            && mount[1] == self.root.to_string_lossy()
                            && mount[2] == self.root.to_string_lossy()
                    }),
                    "Read-only MCP sandbox does not mount the workspace read-only"
                );
            }
            BoundaryAccess::Implementation => {
                anyhow::ensure!(
                    self.resource_socket.is_some() && self.resource_cache.is_some(),
                    "Implementation MCP tools require the application resource broker"
                );
                anyhow::ensure!(
                    self.args.windows(3).any(|mount| {
                        mount[0] == "--bind"
                            && mount[1] == self.root.to_string_lossy()
                            && mount[2] == self.root.to_string_lossy()
                    }),
                    "Implementation MCP sandbox does not mount the assigned clone"
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
    protocol::serve(&config)
}

#[cfg(unix)]
unsafe extern "C" {
    fn geteuid() -> u32;
}
