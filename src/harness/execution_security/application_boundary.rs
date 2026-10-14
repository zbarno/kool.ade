use super::mcp_server::ServerConfig;
use super::session_bridge::SessionBridge;
use crate::execution::{
    contract::{ExecutionModeV1, ResourceBudgetV1},
    docker::images::RuntimeProfile,
    session::ExecutionSession,
};
use crate::harness::{PlanningRequest, ToolAccess};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

mod private_directory;
mod providers;
mod target;
#[cfg(test)]
#[path = "application_boundary/tests.rs"]
mod tests;
pub(crate) use private_directory::PrivateDirectory;
pub(crate) use providers::{CliProvider, ProfileProbeEvidence, ValidatedProviderLaunch};

pub(crate) struct ApplicationBoundary {
    scratch: PrivateDirectory,
    server_name: String,
    app_binary: Option<PathBuf>,
    server_config: Option<PathBuf>,
    access: ToolAccess,
    _session_bridge: Option<SessionBridge>,
    _resource_bridge: Option<Arc<crate::harness::resource_bridge::ResourceBridge>>,
}

impl ApplicationBoundary {
    pub(crate) fn new(request: &PlanningRequest) -> anyhow::Result<Self> {
        crate::harness::validate_review_invocation(request)
            .map_err(|error| anyhow::anyhow!(error.detail()))?;
        #[cfg(test)]
        if crate::harness::uncontained_provider_test_execution_enabled() {
            return Self::new_for_uncontained_test(request);
        }
        let scratch = PrivateDirectory::create("koolade-cli-session")?;
        let requested_access = request.mode.tool_access();
        let app_binary = if requested_access == ToolAccess::None {
            None
        } else {
            // Unit tests run inside a test-harness executable, which does not
            // implement the application's private MCP entry point. Hosted
            // integration tests build the real sibling application binary
            // and provide it explicitly; production always uses itself.
            #[cfg(test)]
            let executable = std::env::var_os("KOOLADE_TEST_APP_BINARY")
                .map(PathBuf::from)
                .map(Ok)
                .unwrap_or_else(std::env::current_exe)?;
            #[cfg(not(test))]
            let executable = std::env::current_exe()?;
            Some(executable.canonicalize()?)
        };
        let mut resource_bridge = None;
        let mut session_bridge = None;
        let mut access = ToolAccess::None;
        let server_config = if requested_access == ToolAccess::BoundedImplementation {
            anyhow::ensure!(
                cfg!(target_os = "linux"),
                "Docker repository tools are supported only on Linux"
            );
            let workspace = request.execution_workspace.clone().ok_or_else(|| {
                anyhow::anyhow!(
                    "Docker implementation requires an application-selected task workspace"
                )
            })?;
            let bridge = Arc::new(
                crate::harness::resource_bridge::ResourceBridge::start_for_task_repository(
                    &request.repo_root,
                    request.runtime_config_source.as_deref(),
                    request.task_id.as_deref(),
                    request.progress_tx.clone(),
                    request.cancel.clone(),
                )?,
            );
            let session = Arc::new(target::connect(
                request,
                workspace.clone(),
                runtime_profile(&request.repo_root),
                default_resource_budget(),
            )?);
            let ipc = SessionBridge::start(
                &scratch.path,
                session,
                workspace.task_id.clone(),
                "implementation".into(),
                request.cancel.clone(),
                Some(bridge.clone()),
            )?;
            let config = ServerConfig::implementation(&ipc.binding, &bridge);
            session_bridge = Some(ipc);
            resource_bridge = Some(bridge);
            access = ToolAccess::BoundedImplementation;
            Some(config)
        } else if requested_access == ToolAccess::ReadOnly {
            if let Some(workspace) = request.execution_workspace.clone() {
                anyhow::ensure!(
                    cfg!(target_os = "linux"),
                    "Docker planning tools are supported only on Linux"
                );
                let session = ExecutionSession::connect_read_only_local(
                    workspace.clone(),
                    &request.repo_root,
                    ExecutionModeV1::PlanningReadOnly,
                    runtime_profile(&request.repo_root),
                    default_resource_budget(),
                )
                .map_err(|error| {
                    anyhow::anyhow!(
                        "Could not establish the requested read-only Docker planning session for the selected task workspace; verify the local Docker Engine and workspace source revision: {error}"
                    )
                })?;
                let ipc = SessionBridge::start(
                    &scratch.path,
                    Arc::new(session),
                    workspace.task_id.clone(),
                    "read_only".into(),
                    request.cancel.clone(),
                    None,
                )?;
                let config = ServerConfig::read_only(&ipc.binding);
                session_bridge = Some(ipc);
                access = ToolAccess::ReadOnly;
                Some(config)
            } else {
                None
            }
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
            _session_bridge: session_bridge,
            _resource_bridge: resource_bridge,
        })
    }

    #[cfg(test)]
    fn new_for_uncontained_test(_request: &PlanningRequest) -> anyhow::Result<Self> {
        Ok(Self {
            scratch: PrivateDirectory::create("koolade-cli-test")?,
            server_name: format!("koolade_{}", uuid::Uuid::new_v4().simple()),
            app_binary: None,
            server_config: None,
            access: ToolAccess::None,
            _session_bridge: None,
            _resource_bridge: None,
        })
    }

    pub(crate) fn working_directory(&self) -> &Path {
        &self.scratch.path
    }

    pub(crate) fn access(&self) -> ToolAccess {
        self.access
    }

    pub(crate) fn resource_bridge(
        &self,
    ) -> Option<&crate::harness::resource_bridge::ResourceBridge> {
        self._resource_bridge.as_deref()
    }

    pub(crate) fn system_policy(&self) -> &'static str {
        match self.access {
            ToolAccess::None => {
                "This turn has no repository tools. Work only from the context supplied by Kool.ad/e."
            }
            ToolAccess::ReadOnly => {
                "Repository reads are available only through Kool.ad/e's read-only MCP tool. The CLI has no direct repository tools. Reads run in an application-owned Docker session with the selected repository mounted read-only, no ambient network, and no host credentials."
            }
            ToolAccess::BoundedImplementation => {
                "Repository access is available only through Kool.ad/e MCP tools. Use koolade_bash for commands, koolade_dependency before package changes or restores, and koolade_resource only for public non-package HTTPS resources. The CLI has no direct repository tools. Commands run in an application-owned Docker session with the assigned clone as the only writable workspace, no ambient network, hidden host credentials, and app-mediated dependency and resource access. Kool.ad/e alone commits, pushes, integrates, and publishes."
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

fn runtime_profile(root: &Path) -> RuntimeProfile {
    if root.join("Cargo.toml").is_file() {
        RuntimeProfile::Rust
    } else if root.join("package.json").is_file() {
        RuntimeProfile::Node
    } else if root.join("pyproject.toml").is_file() || root.join("requirements.txt").is_file() {
        RuntimeProfile::Python
    } else if root.join("go.mod").is_file() {
        RuntimeProfile::Go
    } else {
        RuntimeProfile::General
    }
}

fn default_resource_budget() -> ResourceBudgetV1 {
    ResourceBudgetV1::default()
}
