use super::session_bridge::SessionBridge;
use crate::execution::session::ExecutionSession;
use crate::harness::{PlanningRequest, ToolAccess};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

mod construction;
mod private_directory;
mod providers;
mod runtime_qa_screenshots;
mod target;
#[cfg(test)]
mod test_overrides;
#[cfg(test)]
mod test_routing;
#[cfg(test)]
#[path = "application_boundary/tests.rs"]
mod tests;
#[cfg(test)]
use construction::default_resource_budget;
pub(crate) use private_directory::PrivateDirectory;
pub(crate) use providers::{
    CliProvider, ProfileProbeEvidence, ValidatedProviderLaunch, ensure_claude_update_policy,
};
#[cfg(test)]
pub(crate) use test_overrides::override_test_resource_budgets;
#[cfg(test)]
use test_overrides::resource_budget_override;

#[cfg(test)]
static TEST_APP_BINARY_OVERRIDE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) struct TestAppBinaryOverride(Option<PathBuf>);

#[cfg(test)]
pub(crate) fn override_test_app_binary(path: PathBuf) -> TestAppBinaryOverride {
    let mut current = TEST_APP_BINARY_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    TestAppBinaryOverride(current.replace(path))
}

#[cfg(test)]
pub(crate) use test_routing::auto_remote_session_for_retry_test;

#[cfg(test)]
impl Drop for TestAppBinaryOverride {
    fn drop(&mut self) {
        let mut current = TEST_APP_BINARY_OVERRIDE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *current = self.0.take();
    }
}

#[cfg(test)]
fn test_app_binary_override() -> Option<PathBuf> {
    TEST_APP_BINARY_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

pub(crate) struct ApplicationBoundary {
    scratch: PrivateDirectory,
    server_name: String,
    app_binary: Option<PathBuf>,
    server_config: Option<PathBuf>,
    access: ToolAccess,
    _session_bridge: Option<SessionBridge>,
    _resource_bridge: Option<Arc<crate::harness::resource_bridge::ResourceBridge>>,
    execution_session: Option<Arc<ExecutionSession>>,
    screenshot_review_audit: Option<runtime_qa_screenshots::PreparedReviewImages>,
}

impl ApplicationBoundary {
    pub(crate) fn new(request: &PlanningRequest) -> anyhow::Result<Self> {
        construction::new(request)
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
            execution_session: None,
            screenshot_review_audit: None,
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
            ToolAccess::RuntimeQaScreenshots => {
                "The only available tool is koolade_runtime_qa_screenshot. It returns the actual hash-validated image pixels for a screenshot in this immutable Runtime QA run. Inspect each image you cite. No repository, shell, browser, desktop-control, edit, or publication tools are available."
            }
        }
    }

    pub(crate) fn configure(
        &self,
        provider: CliProvider,
        argv: &mut Vec<String>,
        env: &mut Vec<(String, String)>,
    ) -> anyhow::Result<()> {
        providers::configure(self, provider, argv, env)?;
        if let Some(session) = &self.execution_session {
            session.set_telemetry_attribution(provider.telemetry_id(), None);
        }
        Ok(())
    }

    pub(crate) fn set_telemetry_model(&self, model: Option<&str>) {
        if let Some(session) = &self.execution_session {
            session.set_telemetry_model(model);
        }
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
        if self.access == ToolAccess::RuntimeQaScreenshots {
            return vec!["koolade_runtime_qa_screenshot".to_owned()];
        }
        if self.access == ToolAccess::BoundedImplementation {
            names.extend([
                "koolade_resource".to_owned(),
                "koolade_dependency".to_owned(),
            ]);
        }
        names
    }
}

impl Drop for ApplicationBoundary {
    fn drop(&mut self) {
        if let Some(audit) = &self.screenshot_review_audit {
            runtime_qa_screenshots::merge_inspected(
                &audit.audit_root,
                &audit.allowed_ids,
                &audit.inspected,
            );
        }
    }
}
