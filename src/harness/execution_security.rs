use super::ExecutionMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilesystemScope {
    AssignedCloneAndExplicitAllowlist,
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
    ApplicationControlled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessIsolation {
    BubblewrapNamespaces,
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
                filesystem: FilesystemScope::AssignedCloneAndExplicitAllowlist,
                network: NetworkPolicy::NoAmbientNetwork,
                tool_operations: ToolOperations::SandboxedShellAndBrokeredResources,
                dependency_requests: DependencyPolicy::ApplicationAuthorizationBroker,
                credentials: CredentialPolicy::ApplicationControlled,
                process_isolation: ProcessIsolation::BubblewrapNamespaces,
            } => {
                "Bubblewrap isolates the assigned clone with explicit mounts, no ambient network, brokered dependency requests, and application controlled credentials."
            }
        }
    }
}

static PI_MODES: [ExecutionMode; 7] = ExecutionMode::ALL;
const PI_CAPABILITIES: HarnessCapabilities = HarnessCapabilities {
    policy: ExecutionSecurityPolicy {
        filesystem: FilesystemScope::AssignedCloneAndExplicitAllowlist,
        network: NetworkPolicy::NoAmbientNetwork,
        tool_operations: ToolOperations::SandboxedShellAndBrokeredResources,
        dependency_requests: DependencyPolicy::ApplicationAuthorizationBroker,
        credentials: CredentialPolicy::ApplicationControlled,
        process_isolation: ProcessIsolation::BubblewrapNamespaces,
    },
    modes: &PI_MODES,
};

pub(crate) fn for_harness(id: &str) -> Option<HarnessCapabilities> {
    (id == "pi").then_some(PI_CAPABILITIES)
}

pub(crate) fn implementation_route_available(id: &str) -> bool {
    for_harness(id).is_some_and(HarnessCapabilities::supports_implementation)
}

pub(crate) fn summary_for_harness(id: &str) -> &'static str {
    for_harness(id).map_or(
        "Repository execution is disabled because this CLI has no application owned sandbox policy.",
        HarnessCapabilities::settings_summary,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pi_advertises_a_complete_application_owned_execution_policy() {
        let pi = for_harness("pi").unwrap();
        assert!(pi.supports_implementation());
        assert!(ExecutionMode::ALL.into_iter().all(|mode| pi.supports(mode)));
        assert_eq!(
            pi.policy.process_isolation,
            ProcessIsolation::BubblewrapNamespaces
        );
        for id in ["codex", "claude", "opencode", "copilot", "antigravity"] {
            assert!(for_harness(id).is_none());
            assert!(!implementation_route_available(id));
        }
    }
}
