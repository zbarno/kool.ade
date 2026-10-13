//! Host execution capabilities for the Docker-backed application boundary.

use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

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

const HEALTH_CACHE_TTL: Duration = Duration::from_secs(15);
static HEALTH_CACHE: OnceLock<Mutex<Option<(Instant, RuntimeCapabilities)>>> = OnceLock::new();

impl RuntimeCapabilities {
    /// Return a recent Docker health result. UI callers often ask from a paint
    /// or polling path, so the daemon and image inventory are not queried on
    /// every frame.
    pub fn detect() -> Self {
        let cache = HEALTH_CACHE.get_or_init(|| Mutex::new(None));
        if let Ok(cache) = cache.lock()
            && let Some((checked_at, result)) = *cache
            && checked_at.elapsed() < HEALTH_CACHE_TTL
        {
            return result;
        }
        let result = Self::for_docker_ready(docker_ready());
        if let Ok(mut cache) = cache.lock() {
            *cache = Some((Instant::now(), result));
        }
        result
    }

    pub fn invalidate() {
        if let Some(cache) = HEALTH_CACHE.get()
            && let Ok(mut cache) = cache.lock()
        {
            *cache = None;
        }
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

    fn for_docker_ready(ready: bool) -> Self {
        Self {
            planning_access: if ready {
                PlanningAccess::SandboxedRepositoryReads
            } else {
                PlanningAccess::SuppliedContextOnly
            },
            implementation: ready,
        }
    }

    pub fn implementation_unavailable_message(&self) -> &'static str {
        "Kool.ad/e could not verify a ready Docker execution environment. Start Docker and ensure the approved runtime image is available, then retry. Your planned artifacts are preserved."
    }
}

fn docker_ready() -> bool {
    crate::execution::docker::environment::DockerEnvironmentStatus::detect().ready()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_docker_disables_repository_tools_but_keeps_context_planning() {
        let capabilities = RuntimeCapabilities::for_docker_ready(false);
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
    fn ready_docker_enables_workspace_scoped_modes() {
        let capabilities = RuntimeCapabilities::for_docker_ready(true);
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
    fn unavailable_messages_name_docker_and_preserve_context_planning() {
        let capabilities = RuntimeCapabilities::for_docker_ready(false);
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
