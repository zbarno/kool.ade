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

pub(super) fn new(request: &PlanningRequest) -> anyhow::Result<ApplicationBoundary> {
    crate::harness::validate_review_invocation(request)
        .map_err(|error| anyhow::anyhow!(error.detail()))?;
    #[cfg(test)]
    if crate::harness::uncontained_provider_test_execution_enabled() {
        return ApplicationBoundary::new_for_uncontained_test(request);
    }
    let scratch = super::PrivateDirectory::create("koolade-cli-session")?;
    let requested_access = request.mode.tool_access();
    let app_binary = if requested_access == ToolAccess::None {
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
    let mut access = ToolAccess::None;
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
    let server_config = server_config
        .map(|config| scratch.write_json("server.json", &config))
        .transpose()?;
    Ok(ApplicationBoundary {
        scratch,
        server_name: format!("koolade_{}", uuid::Uuid::new_v4().simple()),
        app_binary,
        server_config,
        access,
        _session_bridge: session_bridge,
        _resource_bridge: resource_bridge,
        execution_session,
        screenshot_review_audit,
    })
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
