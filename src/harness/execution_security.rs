use super::ExecutionMode;

mod application_boundary;
mod mcp_server;
#[cfg(unix)]
mod session_bridge;
#[cfg(windows)]
#[path = "execution_security/session_bridge_windows.rs"]
mod session_bridge;
#[cfg(test)]
pub(crate) use application_boundary::auto_remote_session_for_retry_test;
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
pub(crate) use application_boundary::{
    ApplicationBoundary, CliProvider, ProfileProbeEvidence, ValidatedProviderLaunch,
    ensure_claude_update_policy, opencode_has_custom_provider_settings,
};
#[cfg(test)]
pub(crate) use application_boundary::{override_test_app_binary, override_test_resource_budgets};

/// Confirm that the installed CLI exposes the invocation and isolation
/// controls required by its clean application launch profile.
pub(crate) fn verify_scriptable_invocation(
    provider_id: &str,
    executable: &std::path::Path,
    version: &str,
) -> anyhow::Result<()> {
    let provider = CliProvider::from_id(provider_id)
        .ok_or_else(|| anyhow::anyhow!("unknown CLI provider '{provider_id}'"))?;
    ProfileProbeEvidence::probe(provider, executable, version.to_owned()).map(|_| ())
}
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
                "Supported provider CLIs stay on the host for authentication and model connection, start in a private working directory, and have native repository tools disabled. Every repository operation runs through Kool.ad/e's application-owned MCP session and Docker ExecutionSession, bound to the selected workspace and execution mode. Docker mounts only the assigned clone writable for implementation or the selected repository read-only for planning; containers receive no provider credentials or ambient network. Dependency and public-resource access remains application-mediated. Provider launches and probes remove inherited startup-code, shell-init, loader, Git-config, and IDE-connection variables while retaining provider auth and model transport. Locally visible host hooks, commands, and extra MCP servers are rejected; any local Claude Code managed-settings file or drop-in is rejected, and its host-side MCP shell-prefix override is removed. Codex or Claude account-managed policy may still supply host-side hooks, helper commands, MCP servers, or feature settings; the app cannot preflight cloud-delivered policy, so task execution for both providers is blocked. Claude Code is unavailable under WSL because it can inherit Windows-managed policy that Kool.ad/e cannot inspect."
            }
        }
    }
}

static SUPPORTED_MODES: [ExecutionMode; 8] = ExecutionMode::ALL;
const CLAUDE_TASK_EXECUTION_BLOCK: &str = "Claude Code task execution is unavailable until Kool.ad/e can sandbox organization-managed hooks; the CLI does not provide a supported way to disable those hooks while keeping normal sign-in.";
const CODEX_TASK_EXECUTION_BLOCK: &str = "Codex task execution is unavailable until Kool.ad/e can contain organization-managed hooks; Codex feature-disable flags do not override admin-enforced hook policy.";
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
    supports_mode_contract(id, ExecutionMode::Implementation)
}

pub(crate) fn task_execution_block_reason(id: &str) -> Option<&'static str> {
    let id = id.trim();
    if ["claude", "claude code"]
        .iter()
        .any(|name| id.eq_ignore_ascii_case(name))
    {
        Some(CLAUDE_TASK_EXECUTION_BLOCK)
    } else if ["codex", "codex cli"]
        .iter()
        .any(|name| id.eq_ignore_ascii_case(name))
    {
        Some(CODEX_TASK_EXECUTION_BLOCK)
    } else {
        None
    }
}

/// Whether a provider adapter in this build implements the selected mode.
/// Discovery combines this with the installed CLI's verified launch profile.
pub(crate) fn supports_mode_contract(id: &str, mode: ExecutionMode) -> bool {
    task_execution_block_reason(id).is_none()
        && for_harness(id).is_some_and(|capabilities| capabilities.supports(mode))
}

/// Whether this build has a configured app-owned MCP adapter and execution
/// policy for every public mode and role of a supported CLI. This records the
/// application bridge contract; a live Docker target is probed separately.
pub(crate) fn supports_universal_role_contract(id: &str) -> bool {
    let Some(provider) = CliProvider::from_id(id) else {
        return false;
    };
    if for_harness(id).is_none() {
        return false;
    }
    let profile = provider.profile_requirements();
    if profile.launch_tokens.is_empty() || profile.capabilities.is_empty() {
        return false;
    }
    ExecutionMode::ALL
        .into_iter()
        .all(|mode| supports_mode_contract(id, mode))
        && crate::execution::contract::HarnessRole::ALL
            .into_iter()
            .all(|role| supports_mode_contract(id, role.harness_mode()))
}

pub(crate) fn summary_for_harness(id: &str) -> &'static str {
    if let Some(reason) = task_execution_block_reason(id) {
        return reason;
    }
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
        for id in ["pi", "opencode", "copilot", "antigravity"] {
            let capabilities = for_harness(id).unwrap();
            assert!(capabilities.supports(ExecutionMode::Implementation), "{id}");
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
        assert!(!supports_mode_contract("claude", ExecutionMode::Planning));
        assert!(!supports_mode_contract(
            "claude",
            ExecutionMode::Implementation
        ));
        assert!(!implementation_route_available("claude code"));
        assert!(task_execution_block_reason("claude").is_some());
        assert!(!supports_mode_contract("codex", ExecutionMode::Planning));
        assert!(!supports_mode_contract(
            "Codex CLI",
            ExecutionMode::Implementation
        ));
        assert!(!implementation_route_available("codex"));
        assert!(task_execution_block_reason("codex").is_some());
        assert!(for_harness("unknown").is_none());
        assert!(!implementation_route_available("unknown"));
    }
}
