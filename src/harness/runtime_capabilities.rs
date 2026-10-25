//! Host execution capabilities for the Docker-backed application boundary.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanningAccess {
    SandboxedRepositoryReads,
    SuppliedContextOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeCapabilities {
    pub planning_access: PlanningAccess,
    pub implementation: bool,
}

impl RuntimeCapabilities {
    /// DockerEnvironmentStatus owns the shared TTL cache, so capabilities stay
    /// aligned with explicit Docker refreshes without a second stale cache.
    pub fn detect() -> Self {
        let status = crate::execution::docker::environment::DockerEnvironmentStatus::detect();
        Self::from_docker_status(&status)
    }

    pub fn invalidate() {
        crate::execution::docker::environment::DockerEnvironmentStatus::invalidate();
    }

    pub fn tool_access(&self, mode: crate::harness::ExecutionMode) -> crate::harness::ToolAccess {
        if mode.tool_access() == crate::harness::ToolAccess::ReadOnly
            && self.planning_access == PlanningAccess::SuppliedContextOnly
        {
            crate::harness::ToolAccess::None
        } else {
            mode.tool_access()
        }
    }

    pub fn repository_planning_available(&self) -> bool {
        self.planning_access == PlanningAccess::SandboxedRepositoryReads
    }

    pub fn planning_unavailable_message(&self) -> &'static str {
        "Kool.ad/e could not verify a ready Docker execution environment for read-only repository access. Start Docker and ensure the approved runtime images are available, then retry. Planning can continue from supplied context without repository tools."
    }

    fn for_local_execution_ready(ready: bool) -> Self {
        Self {
            planning_access: if ready {
                PlanningAccess::SandboxedRepositoryReads
            } else {
                PlanningAccess::SuppliedContextOnly
            },
            implementation: ready,
        }
    }

    fn from_docker_status(
        status: &crate::execution::docker::environment::DockerEnvironmentStatus,
    ) -> Self {
        Self::for_local_execution_ready(status.local_execution_ready())
    }

    pub fn implementation_unavailable_message(&self) -> &'static str {
        "Kool.ad/e could not verify a ready Docker execution environment. Start Docker and ensure the approved runtime image is available, then retry. Your planned artifacts are preserved."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_docker_disables_repository_tools_but_keeps_context_planning() {
        let capabilities = RuntimeCapabilities::for_local_execution_ready(false);
        assert_eq!(
            capabilities,
            RuntimeCapabilities {
                planning_access: PlanningAccess::SuppliedContextOnly,
                implementation: false,
            }
        );
        for mode in [
            crate::harness::ExecutionMode::Planning,
            crate::harness::ExecutionMode::TaskGeneration,
            crate::harness::ExecutionMode::Investigation,
        ] {
            assert_eq!(
                capabilities.tool_access(mode),
                crate::harness::ToolAccess::None
            );
        }
        assert_eq!(
            capabilities.tool_access(crate::harness::ExecutionMode::Implementation),
            crate::harness::ToolAccess::BoundedImplementation
        );
    }

    #[test]
    fn ready_local_docker_enables_workspace_scoped_modes() {
        let capabilities = RuntimeCapabilities::for_local_execution_ready(true);
        assert_eq!(
            capabilities,
            RuntimeCapabilities {
                planning_access: PlanningAccess::SandboxedRepositoryReads,
                implementation: true,
            }
        );
        assert!(capabilities.repository_planning_available());
        assert!(capabilities.implementation);
    }

    #[test]
    fn engine_health_without_local_execution_does_not_enable_workspace_access() {
        let desktop = crate::execution::docker::environment::DockerEnvironmentStatus {
            state: crate::execution::docker::environment::DockerEnvironmentState::Ready,
            engine_ready: true,
            message: String::new(),
            desktop_vm: true,
            daemon_version: Some("27.0.0".into()),
            architecture: Some("amd64".into()),
            rootless: Some(true),
            cpu_limits: Some(true),
            memory_limits: Some(true),
            pids_limit: Some(true),
            cgroups_supported: Some(true),
            available_images: Vec::new(),
            disk_available_bytes: Some(1_000_000_000),
            endpoint_fingerprint: None,
            daemon_identity_fingerprint: None,
        };
        assert!(desktop.ready());
        assert!(!desktop.local_execution_ready());

        let capabilities = RuntimeCapabilities::from_docker_status(&desktop);
        assert!(!capabilities.repository_planning_available());
        assert!(!capabilities.implementation);
    }

    #[test]
    fn unavailable_messages_name_docker_and_preserve_context_planning() {
        let capabilities = RuntimeCapabilities::for_local_execution_ready(false);
        assert!(
            capabilities
                .planning_unavailable_message()
                .contains("Docker")
        );
        assert!(
            capabilities
                .planning_unavailable_message()
                .contains("supplied context")
        );
        assert!(
            capabilities
                .implementation_unavailable_message()
                .contains("Docker")
        );
    }
}
