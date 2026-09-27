//! Host execution capabilities. A successful app build does not imply that
//! autonomous execution sandboxes are available on that host.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Linux,
    MacOs,
    Windows,
    Other,
}

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
        let platform = if cfg!(target_os = "linux") {
            Platform::Linux
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Other
        };
        let bwrap_program = std::env::var_os("PACKET_BWRAP_BIN").unwrap_or_else(|| "bwrap".into());
        let bwrap = platform == Platform::Linux
            && std::process::Command::new(bwrap_program)
                .arg("--version")
                .output()
                .is_ok_and(|output| output.status.success());
        Self::for_host(platform, bwrap)
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

    fn for_host(platform: Platform, bwrap: bool) -> Self {
        let sandboxed = platform == Platform::Linux && bwrap;
        Self {
            planning_access: if sandboxed {
                PlanningAccess::SandboxedRepositoryReads
            } else {
                PlanningAccess::SuppliedContextOnly
            },
            implementation: sandboxed,
        }
    }

    pub fn implementation_unavailable_message(&self) -> &'static str {
        "Implementation is unavailable because Packet cannot establish its required filesystem sandbox on this host. Planning remains available, and your planned artifacts are preserved."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_matrix_keeps_planning_available_and_disables_unsupported_execution() {
        for platform in [Platform::MacOs, Platform::Windows, Platform::Other] {
            assert_eq!(
                RuntimeCapabilities::for_host(platform, false),
                RuntimeCapabilities {
                    planning_access: PlanningAccess::SuppliedContextOnly,
                    implementation: false,
                }
            );
        }
        assert_eq!(
            RuntimeCapabilities::for_host(Platform::Linux, false).planning_access,
            PlanningAccess::SuppliedContextOnly
        );
        assert_eq!(
            RuntimeCapabilities::for_host(Platform::Linux, true),
            RuntimeCapabilities {
                planning_access: PlanningAccess::SandboxedRepositoryReads,
                implementation: true,
            }
        );
    }

    #[test]
    fn unsupported_hosts_keep_read_only_modes_context_only() {
        let capabilities = RuntimeCapabilities::for_host(Platform::MacOs, false);
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
