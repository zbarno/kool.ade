use super::mcp_server::ServerConfig;
use crate::harness::{PlanningRequest, ToolAccess};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

mod providers;
pub(crate) use providers::CliProvider;

pub(crate) struct ApplicationBoundary {
    scratch: PrivateDirectory,
    server_name: String,
    app_binary: Option<PathBuf>,
    server_config: Option<PathBuf>,
    access: ToolAccess,
    _resource_bridge: Option<crate::harness::resource_bridge::ResourceBridge>,
    _sandbox: Option<crate::harness::pi_sandbox::Sandbox>,
    _planning_sandbox: Option<crate::harness::pi_sandbox::PlanningSandbox>,
}

impl ApplicationBoundary {
    pub(crate) fn new(request: &PlanningRequest) -> anyhow::Result<Self> {
        #[cfg(test)]
        if crate::harness::uncontained_provider_test_execution_enabled() {
            return Self::new_for_uncontained_test(request);
        }
        anyhow::ensure!(
            cfg!(target_os = "linux"),
            "Kool.ad/e requires Bubblewrap to run supported CLI tools safely"
        );
        let scratch = PrivateDirectory::create("koolade-cli-session")?;
        let access = request.mode.tool_access();
        let app_binary = (access != ToolAccess::None)
            .then(std::env::current_exe)
            .transpose()?
            .map(|path| path.canonicalize())
            .transpose()?;
        let mut resource_bridge = None;
        let mut sandbox = None;
        let mut planning_sandbox = None;
        let server_config = if access == ToolAccess::BoundedImplementation {
            let bridge =
                crate::harness::resource_bridge::ResourceBridge::start_for_task_repository(
                    &request.repo_root,
                    request.runtime_config_source.as_deref(),
                    request.task_id.as_deref(),
                    request.progress_tx.clone(),
                    request.cancel.clone(),
                )?;
            let mut implementation = crate::harness::pi_sandbox::Sandbox::new_for_task_repository(
                &request.repo_root,
                request
                    .runtime_config_source
                    .as_deref()
                    .unwrap_or(&request.repo_root),
            )?;
            implementation.mount_resource_cache(bridge.cache_path())?;
            implementation.mount_npm_cache_with_snapshot(
                bridge.npm_cache_path(),
                bridge.npm_index_snapshot_path(),
            )?;
            implementation.mount_cargo_cache(bridge.cargo_cache_path())?;
            let config = ServerConfig::implementation(&implementation, &bridge);
            sandbox = Some(implementation);
            resource_bridge = Some(bridge);
            Some(config)
        } else if access == ToolAccess::ReadOnly {
            let reads = crate::harness::pi_sandbox::PlanningSandbox::new_for_application(
                &request.repo_root,
            )?;
            let config = ServerConfig::read_only(&reads);
            planning_sandbox = Some(reads);
            Some(config)
        } else {
            None
        };
        let server_config = server_config
            .map(|config| scratch.write_json("server.json", &config))
            .transpose()?;
        Ok(Self {
            scratch,
            server_name: format!("koolade_{}", uuid::Uuid::new_v4().simple()),
            app_binary,
            server_config,
            access,
            _resource_bridge: resource_bridge,
            _sandbox: sandbox,
            _planning_sandbox: planning_sandbox,
        })
    }

    #[cfg(test)]
    fn new_for_uncontained_test(request: &PlanningRequest) -> anyhow::Result<Self> {
        Ok(Self {
            scratch: PrivateDirectory::create("koolade-cli-test")?,
            server_name: format!("koolade_{}", uuid::Uuid::new_v4().simple()),
            app_binary: None,
            server_config: None,
            access: request.mode.tool_access(),
            _resource_bridge: None,
            _sandbox: None,
            _planning_sandbox: None,
        })
    }

    pub(crate) fn working_directory(&self) -> &Path {
        &self.scratch.path
    }

    pub(crate) fn system_policy(&self) -> &'static str {
        match self.access {
            ToolAccess::None => {
                "This turn has no repository tools. Work only from the context supplied by Kool.ad/e."
            }
            ToolAccess::ReadOnly => {
                "Repository reads are available only through Kool.ad/e's read-only MCP tool. The CLI has no direct repository tools. Its reads run in Bubblewrap with host home directories, credentials, writes, and network unavailable."
            }
            ToolAccess::BoundedImplementation => {
                "Repository access is available only through Kool.ad/e MCP tools. Use koolade_bash for commands, koolade_dependency before package changes or restores, and koolade_resource only for public non-package HTTPS resources. The CLI has no direct repository tools. Commands run in Bubblewrap with the assigned clone as the only persistent writable location, no ambient network, hidden host credentials, and app-mediated dependency and resource access. Kool.ad/e alone commits, pushes, integrates, and publishes."
            }
        }
    }

    pub(crate) fn configure(
        &self,
        provider: CliProvider,
        argv: &mut Vec<String>,
        env: &mut Vec<(String, String)>,
    ) -> anyhow::Result<()> {
        providers::configure(self, provider, argv, env)
    }

    fn server_args(&self) -> anyhow::Result<Option<Vec<String>>> {
        let Some(config) = &self.server_config else {
            return Ok(None);
        };
        let executable = self
            .app_binary
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Application MCP server executable is unavailable"))?;
        Ok(Some(vec![
            executable.to_string_lossy().into_owned(),
            "--internal-mcp-server".into(),
            config.to_string_lossy().into_owned(),
        ]))
    }

    pub(super) fn mcp_server_name(&self) -> &str {
        &self.server_name
    }

    pub(super) fn mcp_tool_names(&self) -> Vec<String> {
        if self.server_config.is_none() {
            return Vec::new();
        }
        let mut names = vec!["koolade_bash".to_owned()];
        if self.access == ToolAccess::BoundedImplementation {
            names.extend([
                "koolade_resource".to_owned(),
                "koolade_dependency".to_owned(),
            ]);
        }
        names
    }
}

struct PrivateDirectory {
    path: PathBuf,
}

impl PrivateDirectory {
    fn create(prefix: &str) -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        create_private_directory(&path)?;
        Ok(Self { path })
    }

    fn write_json<T: serde::Serialize>(&self, name: &str, value: &T) -> anyhow::Result<PathBuf> {
        self.write_file(name, &serde_json::to_vec(value)?)
    }

    fn write_file(&self, name: &str, contents: &[u8]) -> anyhow::Result<PathBuf> {
        let path = self.path.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            set_mode(parent, 0o700)?;
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        set_private_file_mode(&mut options);
        let mut file = options.open(&path)?;
        file.write_all(contents)?;
        set_mode(&path, 0o600)?;
        Ok(path)
    }

    fn create_directory(&self, name: &str) -> anyhow::Result<PathBuf> {
        let path = self.path.join(name);
        fs::create_dir_all(&path)?;
        set_mode(&path, 0o700)?;
        Ok(path)
    }
}

impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(path)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> anyhow::Result<()> {
    fs::create_dir(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(options: &mut fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_private_file_mode(_options: &mut fs::OpenOptions) {}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> anyhow::Result<()> {
    Ok(())
}
