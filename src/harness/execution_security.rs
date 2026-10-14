use super::ExecutionMode;

mod application_boundary;
mod mcp_server;
mod session_bridge;
#[cfg(test)]
pub(crate) fn request_remote_dependency_over_application_bridge(
    scratch: &std::path::Path,
    session: std::sync::Arc<crate::execution::session::ExecutionSession>,
    task_id: String,
    resource_bridge: Option<std::sync::Arc<crate::harness::resource_bridge::ResourceBridge>>,
    need: crate::harness::DependencyNeed,
) -> anyhow::Result<session_bridge::DependencyRelayStartResponse> {
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let bridge = session_bridge::SessionBridge::start(
        scratch,
        session,
        task_id,
        "implementation".into(),
        cancel,
        resource_bridge,
    )?;
    session_bridge::request_dependency(&scratch.join("mcp-config.json"), &bridge.binding, need)
}

/// Inherited variables that can inject startup code, choose arbitrary Git
/// configuration, or redirect developer shells and IDE integrations into a CLI.
/// Provider authentication/model transport variables are intentionally retained.
pub(crate) const UNSAFE_PROVIDER_LAUNCH_ENV: &[&str] = &[
    "NODE_OPTIONS",
    "NODE_PATH",
    "BUN_OPTIONS",
    "BUN_CONFIG",
    "BASH_ENV",
    "ENV",
    "PYTHONPATH",
    "PYTHONHOME",
    "PYTHONSTARTUP",
    "RUBYOPT",
    "PERL5OPT",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "GIT_CONFIG",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "VSCODE_IPC_HOOK",
    "VSCODE_IPC_HOOK_CLI",
    "VSCODE_GIT_ASKPASS_MAIN",
    "VSCODE_GIT_ASKPASS_NODE",
    "VSCODE_GIT_IPC_HANDLE",
    "ELECTRON_RUN_AS_NODE",
    "CLAUDE_CODE_SHELL_PREFIX",
];

pub(crate) use application_boundary::PrivateDirectory;
#[cfg(test)]
pub(crate) use application_boundary::override_test_app_binary;
pub(crate) use application_boundary::{
    ApplicationBoundary, CliProvider, ProfileProbeEvidence, ValidatedProviderLaunch,
};
pub(crate) use mcp_server::serve as serve_mcp_server;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilesystemScope {
    PrivateCliDirectoryAndAssignedCloneAllowlist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkPolicy {
    NoAmbientNetwork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolOperations {
    SandboxedShellAndBrokeredResources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DependencyPolicy {
    ApplicationAuthorizationBroker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialPolicy {
    ProviderManagedButUnavailableToRepositoryCommands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessIsolation {
    DockerExecutionSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecutionSecurityPolicy {
    pub filesystem: FilesystemScope,
    pub network: NetworkPolicy,
    pub tool_operations: ToolOperations,
    pub dependency_requests: DependencyPolicy,
    pub credentials: CredentialPolicy,
    pub process_isolation: ProcessIsolation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HarnessCapabilities {
    pub policy: ExecutionSecurityPolicy,
    modes: &'static [ExecutionMode],
}

impl HarnessCapabilities {
    pub(crate) fn supports(self, mode: ExecutionMode) -> bool {
        self.modes.contains(&mode)
    }

    pub(crate) fn supports_implementation(self) -> bool {
        self.supports(ExecutionMode::Implementation)
    }

    pub(crate) fn settings_summary(self) -> &'static str {
        match self.policy {
            ExecutionSecurityPolicy {
                filesystem: FilesystemScope::PrivateCliDirectoryAndAssignedCloneAllowlist,
                network: NetworkPolicy::NoAmbientNetwork,
                tool_operations: ToolOperations::SandboxedShellAndBrokeredResources,
                dependency_requests: DependencyPolicy::ApplicationAuthorizationBroker,
                credentials: CredentialPolicy::ProviderManagedButUnavailableToRepositoryCommands,
                process_isolation: ProcessIsolation::DockerExecutionSession,
            } => {
                "Supported provider CLIs stay on the host for authentication and model connection, start in a private working directory, and have native repository tools disabled. Every repository operation runs through Kool.ad/e's application-owned MCP session and Docker ExecutionSession, bound to the selected workspace and execution mode. Docker mounts only the assigned clone writable for implementation or the selected repository read-only for planning; containers receive no provider credentials or ambient network. Dependency and public-resource access remains application-mediated. Provider launches and probes remove inherited startup-code, shell-init, loader, Git-config, and IDE-connection variables while retaining provider auth and model transport. Locally visible host hooks, commands, and extra MCP servers are rejected; any local Claude Code managed-settings file or drop-in is rejected, and its host-side MCP shell-prefix override is removed. Codex or Claude account-managed policy may still supply host-side hooks, helper commands, MCP servers, or feature settings; the app cannot preflight cloud-delivered policy. Claude Code is unavailable under WSL because it can inherit Windows-managed policy that Kool.ad/e cannot inspect."
            }
        }
    }
}

static SUPPORTED_MODES: [ExecutionMode; 7] = ExecutionMode::ALL;
const APPLICATION_CAPABILITIES: HarnessCapabilities = HarnessCapabilities {
    policy: ExecutionSecurityPolicy {
        filesystem: FilesystemScope::PrivateCliDirectoryAndAssignedCloneAllowlist,
        network: NetworkPolicy::NoAmbientNetwork,
        tool_operations: ToolOperations::SandboxedShellAndBrokeredResources,
        dependency_requests: DependencyPolicy::ApplicationAuthorizationBroker,
        credentials: CredentialPolicy::ProviderManagedButUnavailableToRepositoryCommands,
        process_isolation: ProcessIsolation::DockerExecutionSession,
    },
    modes: &SUPPORTED_MODES,
};

pub(crate) fn for_harness(id: &str) -> Option<HarnessCapabilities> {
    let supported = [
        "pi",
        "codex",
        "claude",
        "claude code",
        "antigravity",
        "opencode",
        "open code",
        "copilot",
        "copilot cli",
    ];
    supported
        .iter()
        .any(|name| id.trim().eq_ignore_ascii_case(name))
        .then_some(APPLICATION_CAPABILITIES)
}

pub(crate) fn implementation_route_available(id: &str) -> bool {
    for_harness(id).is_some_and(HarnessCapabilities::supports_implementation)
}

pub(crate) fn summary_for_harness(id: &str) -> &'static str {
    for_harness(id).map_or(
        "Repository execution is disabled because this CLI has no application-owned sandbox policy.",
        HarnessCapabilities::settings_summary,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_cli_advertises_the_shared_application_owned_policy() {
        for id in [
            "pi",
            "codex",
            "claude",
            "opencode",
            "copilot",
            "antigravity",
        ] {
            let capabilities = for_harness(id).unwrap();
            assert!(capabilities.supports_implementation(), "{id}");
            assert!(
                ExecutionMode::ALL
                    .into_iter()
                    .all(|mode| capabilities.supports(mode))
            );
            assert_eq!(
                capabilities.policy.process_isolation,
                ProcessIsolation::DockerExecutionSession
            );
            assert!(implementation_route_available(id), "{id}");
        }
        assert!(for_harness("unknown").is_none());
        assert!(!implementation_route_available("unknown"));
    }
}
