//! Host execution capabilities. A successful app build does not imply that
//! autonomous execution sandboxes are available on that host.

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
    pub fn detect() -> Self {
        let bwrap_program = std::env::var_os("PACKET_BWRAP_BIN").unwrap_or_else(|| "bwrap".into());
        let bwrap = cfg!(target_os = "linux")
            && std::process::Command::new(bwrap_program)
                .args([
                    "--die-with-parent",
                    "--unshare-user",
                    "--unshare-net",
                    "--ro-bind",
                    "/",
                    "/",
                    "--",
                    "/bin/true",
                ])
                .output()
                .is_ok_and(|output| output.status.success());
        Self::for_host(bwrap)
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
        "Packet needs Bubblewrap to run planning safely. Install the bubblewrap package, ensure Linux user namespaces are available, then retry. No planning agent was started."
    }

    fn for_host(bwrap: bool) -> Self {
        Self {
            planning_access: if bwrap {
                PlanningAccess::SandboxedRepositoryReads
            } else {
                PlanningAccess::SuppliedContextOnly
            },
            implementation: bwrap,
        }
    }

    pub fn implementation_unavailable_message(&self) -> &'static str {
        "Packet needs Bubblewrap to run implementation safely. Install the bubblewrap package, ensure Linux user namespaces are available, then retry. Your planned artifacts are preserved."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_bubblewrap_disables_repository_planning_and_execution() {
        assert_eq!(
            RuntimeCapabilities::for_host(false),
            RuntimeCapabilities {
                planning_access: PlanningAccess::SuppliedContextOnly,
                implementation: false,
            }
        );
        assert_eq!(
            RuntimeCapabilities::for_host(true),
            RuntimeCapabilities {
                planning_access: PlanningAccess::SandboxedRepositoryReads,
                implementation: true,
            }
        );
    }

    #[test]
    fn bubblewrap_setup_messages_name_the_prerequisite_and_next_step() {
        let capabilities = RuntimeCapabilities::for_host(false);
        assert!(
            capabilities
                .planning_unavailable_message()
                .contains("Bubblewrap")
        );
        assert!(
            capabilities
                .planning_unavailable_message()
                .contains("retry")
        );
        assert!(
            capabilities
                .implementation_unavailable_message()
                .contains("user namespaces")
        );
    }

    #[test]
    fn repository_planning_requires_the_sandbox() {
        assert!(!RuntimeCapabilities::for_host(false).repository_planning_available());
        assert!(RuntimeCapabilities::for_host(true).repository_planning_available());
    }

    #[test]
    fn missing_bubblewrap_keeps_read_only_modes_context_only() {
        let capabilities = RuntimeCapabilities::for_host(false);
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
}
