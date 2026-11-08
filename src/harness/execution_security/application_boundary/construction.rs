use std::sync::Arc;

use super::super::mcp_server::ServerConfig;
use super::{ApplicationBoundary, SessionBridge, ToolAccess, runtime_qa_screenshots, target};
use crate::{
    execution::{
        contract::{ExecutionModeV1, ResourceBudgetV1},
        docker::images::RuntimeProfile,
        session::ExecutionSession,
    },
    harness::PlanningRequest,
};

#[cfg(test)]
#[path = "construction/tests.rs"]
mod tests;

pub(super) fn new(request: &PlanningRequest) -> anyhow::Result<ApplicationBoundary> {
    crate::harness::validate_review_invocation(request)
        .map_err(|error| anyhow::anyhow!(error.detail()))?;
    #[cfg(test)]
    if crate::harness::uncontained_provider_test_execution_enabled() {
        return ApplicationBoundary::new_for_uncontained_test(request);
    }
    let approved_project_guidance = if mode_uses_project_guidance(request.mode) {
        crate::persistence::project_guidance::load_for_root(&request.repo_root)?
    } else {
        None
    };
    let scratch = super::PrivateDirectory::create("koolade-cli-session")?;
    let requested_access = request.mode.tool_access();
    if let Some(computer_use) = request.computer_use.as_ref() {
        validate_computer_use_invocation(
            computer_use,
            request.mode,
            request.task_id.as_deref(),
            request
                .prompt_manifest
                .as_ref()
                .map(|manifest| manifest.role_version.as_str()),
            request.execution_workspace.as_ref(),
        )?;
    }
    let app_binary = if requested_access == ToolAccess::None && request.computer_use.is_none() {
        None
    } else {
        #[cfg(test)]
        let executable = super::test_app_binary_override()
            .map(Ok)
            .unwrap_or_else(std::env::current_exe)?;
        #[cfg(not(test))]
        let executable = std::env::current_exe()?;
        Some(executable.canonicalize()?)
    };
    let mut resource_bridge = None;
    let mut session_bridge = None;
    let mut execution_session = None;
    let mut screenshot_review_audit = None;
    let mut computer_use_bridge: Option<super::super::computer_use_bridge::ComputerUseBridge> =
        None;
    let mut access = ToolAccess::None;
    if let Some(computer_use) = request.computer_use.as_ref() {
        let task_id = request.task_id.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Computer Use requires an application-selected task identity")
        })?;
        anyhow::ensure!(
            computer_use.caller.identity.task_id == task_id,
            "Computer Use authority does not match the selected task"
        );
        #[cfg(unix)]
        {
            let bridge = super::super::computer_use_bridge::ComputerUseBridge::start(
                &scratch.path,
                computer_use.clone(),
                request.cancel.clone(),
                request.progress_tx.clone(),
            )?;
            computer_use_bridge = Some(bridge);
        }
        #[cfg(windows)]
        anyhow::bail!("Computer Use MCP is not supported on this host");
    }
    let server_config = if requested_access == ToolAccess::RuntimeQaScreenshots {
        let identity = request
            .review_identity
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("runtime QA visual review has no reviewer identity"))?;
        anyhow::ensure!(
            request
                .prompt_manifest
                .as_ref()
                .is_some_and(|manifest| manifest.role_version == "runtime_qa/v3")
                && identity.runtime_qa_screenshot_review.is_some(),
            "screenshot tools are available only to an independent Runtime QA reviewer"
        );
        let task_id = request
            .task_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("runtime QA visual review has no task identity"))?;
        let review = identity.runtime_qa_screenshot_review.as_ref().unwrap();
        let prepared = runtime_qa_screenshots::PreparedReviewImages::new(
            &scratch,
            identity,
            task_id,
            &review.workspace_id,
            &review.source_sha,
        )?;
        let config = prepared.config.clone();
        screenshot_review_audit = Some(prepared);
        access = ToolAccess::RuntimeQaScreenshots;
        Some(config)
    } else if requested_access == ToolAccess::BoundedImplementation {
        anyhow::ensure!(
            cfg!(target_os = "linux"),
            "Docker repository tools are supported only on Linux"
        );
        let workspace = request.execution_workspace.clone().ok_or_else(|| {
            anyhow::anyhow!("Docker implementation requires an application-selected task workspace")
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
            default_resource_budget(request.task_id.as_deref()),
        )?);
        execution_session = Some(session.clone());
        bridge.bind_execution_session(session.clone())?;
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
    } else if requested_access == ToolAccess::InvestigationBroker {
        if let (Some(workspace), Some(authorization)) = (
            request.execution_workspace.as_ref(),
            request.investigation_authorization.clone(),
        ) {
            anyhow::ensure!(
                authorization.task_id == workspace.task_id
                    && authorization.project_id == workspace.project_id,
                "Manager investigation authorization does not match the selected task"
            );
            let binding = super::super::session_bridge::SessionBinding {
                session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
                capability_id: uuid::Uuid::new_v4().hyphenated().to_string(),
                task_id: workspace.task_id.clone(),
                workspace_id: authorization.workspace_id.clone(),
                access: "investigation_broker".into(),
                target_id: "local".into(),
                target_arch: None,
            };
            let config = ServerConfig::investigation_broker(&binding, authorization);
            access = ToolAccess::InvestigationBroker;
            Some(config)
        } else {
            None
        }
    } else if requested_access == ToolAccess::ReadOnly {
        if let Some(workspace) = request.execution_workspace.clone() {
            anyhow::ensure!(
                cfg!(target_os = "linux"),
                "Docker repository tools are supported only on Linux"
            );
            let session = ExecutionSession::connect_read_only_local(
                workspace.clone(),
                &request.repo_root,
                ExecutionModeV1::PlanningReadOnly,
                runtime_profile(&request.repo_root),
                default_resource_budget(request.task_id.as_deref()),
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
    let server_config = if let Some(computer_use) = &computer_use_bridge {
        let binding = super::super::mcp_server::ComputerUseBridgeConfigV1 {
            socket_path: computer_use.binding.socket_path.clone(),
            bridge_session_id: computer_use.binding.bridge_session_id.clone(),
            capability_id: computer_use.binding.capability_id.clone(),
            task_id: computer_use.binding.task_id.clone(),
        };
        Some(match server_config {
            Some(mut config) => {
                config.attach_computer_use(binding)?;
                config
            }
            None => super::super::mcp_server::ServerConfig::computer_use_only(
                task_id_from_request(request)?,
                binding,
            ),
        })
    } else {
        server_config
    };
    let server_config = server_config
        .map(|config| scratch.write_json("server.json", &config))
        .transpose()?;
    Ok(ApplicationBoundary {
        scratch,
        server_name: format!("koolade_{}", uuid::Uuid::new_v4().simple()),
        app_binary,
        server_config,
        access,
        profile: request.mode.profile(),
        _session_bridge: session_bridge,
        _computer_use_bridge: computer_use_bridge,
        _resource_bridge: resource_bridge,
        execution_session,
        screenshot_review_audit,
        approved_project_guidance,
        codex_auth_strategy: std::sync::Mutex::new(None),
    })
}

fn validate_computer_use_invocation(
    access: &crate::harness::ComputerUseMcpAccessV1,
    mode: crate::harness::ExecutionMode,
    task_id: Option<&str>,
    prompt_role: Option<&str>,
    workspace: Option<&crate::execution::contract::WorkspaceRef>,
) -> anyhow::Result<()> {
    let caller = &access.caller;
    caller.identity.validate().map_err(anyhow::Error::msg)?;
    let task_id = task_id.ok_or_else(|| {
        anyhow::anyhow!("Computer Use requires an application-selected task identity")
    })?;
    anyhow::ensure!(
        caller.identity.task_id == task_id,
        "Computer Use authority does not match the selected task"
    );
    let role_id = prompt_role
        .ok_or_else(|| anyhow::anyhow!("Computer Use requires the app-selected role manifest"))?;
    anyhow::ensure!(
        caller.role_id == role_id,
        "Computer Use authority does not match the app-selected role"
    );
    match (mode, role_id) {
        (crate::harness::ExecutionMode::Implementation, "implementer/v1" | "repair/v1") => {
            let workspace = workspace.ok_or_else(|| {
                anyhow::anyhow!("Computer Use implementation requires the selected task workspace")
            })?;
            anyhow::ensure!(
                workspace.task_id == task_id && workspace.project_id == caller.identity.project_id,
                "Computer Use authority does not match the selected project workspace"
            );
        }
        _ => anyhow::bail!("Computer Use is unavailable to this role or execution mode"),
    }
    Ok(())
}

fn task_id_from_request(request: &PlanningRequest) -> anyhow::Result<String> {
    request
        .task_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Computer Use requires a task identity"))
}

pub(super) fn mode_uses_project_guidance(mode: crate::harness::ExecutionMode) -> bool {
    matches!(
        mode,
        crate::harness::ExecutionMode::Planning
            | crate::harness::ExecutionMode::TaskGeneration
            | crate::harness::ExecutionMode::Investigation
            | crate::harness::ExecutionMode::Implementation
    )
}

fn runtime_profile(root: &std::path::Path) -> RuntimeProfile {
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

pub(super) fn default_resource_budget(task_id: Option<&str>) -> ResourceBudgetV1 {
    #[cfg(test)]
    if let Some(budget) = task_id.and_then(super::resource_budget_override) {
        return budget;
    }
    let _ = task_id;
    ResourceBudgetV1::default()
}
