//! The external-AI-harness boundary (SPECIFICATION.md §14–§15).
//!
//! Named-file module layout:
//! * `api.rs`          — trait, request/outcome/envelope types
//! * `pi_harness.rs`   — the Pi CLI implementation (only MVP backend)
//! * `pi_proc.rs`      — child-process supervision primitives
//! * `pi_events.rs`    — NDJSON event-stream folding
//! * `pi_extract.rs`   — JSON-block extraction from final prose

pub mod antigravity_harness;
pub mod claude_harness;
pub mod codex_harness;
pub mod copilot_harness;
pub(crate) mod dependency_authorization;
pub(crate) mod execution_security;
pub mod live_preview;
pub mod opencode_harness;
pub mod pi_events;
pub mod pi_extract;
pub mod pi_harness;
pub mod pi_proc;
pub(crate) mod pi_sandbox;
pub mod responses;
pub mod runtime_capabilities;

pub const CODEX_HARNESS_ENV: &str = "KOOLADE_HARNESS";

pub(crate) fn implementation_route_available(harness: &str) -> bool {
    execution_security::implementation_route_available(harness)
}

pub(crate) fn require_application_implementation_boundary(
    harness: &str,
    request: &PlanningRequest,
) -> Result<(), crate::error::AppError> {
    #[cfg(test)]
    if provider_test_override::enabled() {
        return Ok(());
    }
    let policy_id = match harness {
        "Claude Code" => "claude",
        "Copilot CLI" => "copilot",
        other => other,
    };
    if !execution_security::for_harness(policy_id)
        .is_some_and(|capabilities| capabilities.supports(request.mode))
    {
        return Err(crate::error::AppError::HarnessFailed {
            reason: format!(
                "{harness} repository access is unavailable because it cannot run inside Kool.ad/e's application-owned Linux sandbox. Select Pi on a host with Bubblewrap. No CLI was started."
            ),
            stderr_tail: String::new(),
        });
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn with_uncontained_provider_test_execution<T>(run: impl FnOnce() -> T) -> T {
    provider_test_override::with_enabled(run)
}

#[cfg(test)]
mod provider_test_override {
    use std::cell::Cell;

    std::thread_local! {
        static ENABLED: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn enabled() -> bool {
        ENABLED.with(Cell::get)
    }

    pub(super) fn with_enabled<T>(run: impl FnOnce() -> T) -> T {
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                ENABLED.with(|enabled| enabled.set(self.0));
            }
        }
        let previous = ENABLED.with(|enabled| enabled.replace(true));
        let _restore = Restore(previous);
        run()
    }
}

/// Return a saved operator path, validating the filesystem part before a
/// provider probe or task launch uses it. Provider-specific probes still
/// validate the CLI identity and capabilities.
pub(crate) fn manual_executable_path(
    id: &str,
) -> Result<Option<std::path::PathBuf>, crate::error::AppError> {
    let (settings, _) = crate::persistence::harness_settings::load();
    manual_executable_path_from(&settings, id)
}

pub(crate) fn manual_executable_path_from(
    settings: &crate::persistence::harness_settings::HarnessSettings,
    id: &str,
) -> Result<Option<std::path::PathBuf>, crate::error::AppError> {
    let Some(value) = settings.manual_executable_paths.get(id) else {
        return Ok(None);
    };
    let path = std::path::PathBuf::from(value);
    let metadata =
        std::fs::metadata(&path).map_err(|_| crate::error::AppError::HarnessNotFound {
            detail: format!(
                "Configured {id} executable path does not exist: {}",
                path.display()
            ),
        })?;
    if !metadata.is_file() || !is_executable_file(&metadata) {
        return Err(crate::error::AppError::HarnessNotFound {
            detail: format!(
                "Configured {id} path is not an executable file: {}",
                path.display()
            ),
        });
    }
    Ok(Some(path))
}

fn is_executable_file(metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        true
    }
}

#[cfg(test)]
#[path = "harness/tests.rs"]
mod tests;

pub use antigravity_harness::AntigravityHarness;
pub use api::{
    ActivityTelemetry, AiHarness, ApplicationAction, DependencyAuthorizationScope,
    DependencyAuthorizationSource, DependencyDecision, DependencyFailureCategory, DependencyKind,
    DependencyNeed, DependencyPackageIdentity, DependencyPreparationStatus,
    DependencyPreparationTelemetry, DependencyRequest, DependencyRequestStatus,
    DependencyRetryResult, DocumentUpdate, ExecutionMode, HarnessOutcome, LivePost, LiveProgress,
    ModelCallUsage, PackageEcosystem, PlanningRequest, PlanningTaskDraft, PlanningTaskOffer,
    RequestedAction, RetrievalPlan, ToolAccess, TurnEnvelope, TurnItem, TurnItemUpdate,
};
pub use claude_harness::ClaudeHarness;
pub use codex_harness::CodexHarness;
pub use copilot_harness::CopilotHarness;
pub use opencode_harness::OpenCodeHarness;
pub use pi_harness::PiHarness;

mod api;
mod nuget_audit;
mod resource_bridge;

pub(crate) fn nuget_audit_cache_path() -> anyhow::Result<std::path::PathBuf> {
    nuget_audit::cache_path()
}

pub(crate) fn refresh_nuget_audit_cache(timeout: std::time::Duration) -> anyhow::Result<()> {
    nuget_audit::refresh(timeout)
}

pub(crate) fn prepared_npm_cache_path(
    worktree: &std::path::Path,
) -> anyhow::Result<std::path::PathBuf> {
    resource_bridge::prepared_npm_cache_path(worktree)
}

pub(crate) fn prepared_cargo_cache_path() -> anyhow::Result<std::path::PathBuf> {
    resource_bridge::prepared_cargo_cache_path()
}

pub(crate) fn publish_npm_cache_index_snapshot(
    cache_root: &std::path::Path,
    snapshot_root: &std::path::Path,
) -> anyhow::Result<()> {
    resource_bridge::publish_npm_cache_index_snapshot(cache_root, snapshot_root)
}

pub(crate) fn dependency_decision_allowed(
    need: &DependencyNeed,
    decision: DependencyDecision,
) -> bool {
    resource_bridge::dependency::decision_allowed(need, decision)
}

pub(crate) fn manager_dependency_decision_allowed(
    need: &DependencyNeed,
    decision: DependencyDecision,
) -> bool {
    resource_bridge::dependency::manager_decision_allowed(need, decision)
}
