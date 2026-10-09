use super::ExecutionMode;

mod application_boundary;
mod mcp_server;

pub(crate) use application_boundary::{ApplicationBoundary, CliProvider};
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
    BubblewrapForRepositoryCommands,
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
                process_isolation: ProcessIsolation::BubblewrapForRepositoryCommands,
            } => {
                "Pi runs inside Bubblewrap. Other supported provider CLIs stay on the host for authentication and model connection, start in a private working directory, and have native repository tools disabled by Kool.ad/e. Repository commands run only through Kool.ad/e's per-run MCP bridge inside Bubblewrap, with explicit mounts, no ambient network or host credentials, and brokered resource and dependency requests. Host-side CLIs start without NODE_OPTIONS so Node startup code cannot run before restrictions apply. Locally visible host hooks, commands, and extra MCP servers are rejected; any local Claude Code managed-settings file or drop-in is rejected, and its host-side MCP shell-prefix override is removed. Codex or Claude account-managed policy may still supply host-side hooks, helper commands, MCP servers, or feature settings; the app cannot preflight cloud-delivered policy. Claude Code is unavailable under WSL because it can inherit Windows-managed policy that Kool.ad/e cannot inspect."
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
        process_isolation: ProcessIsolation::BubblewrapForRepositoryCommands,
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
                ProcessIsolation::BubblewrapForRepositoryCommands
            );
            assert!(implementation_route_available(id), "{id}");
        }
        assert!(for_harness("unknown").is_none());
        assert!(!implementation_route_available("unknown"));
    }
}
