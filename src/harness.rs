//! The external-AI-harness boundary (SPECIFICATION.md §14–§15).
//!
//! Named-file module layout:
//! * `api.rs`          — trait, request/outcome/envelope types
//! * `pi_harness.rs`   — the Pi CLI implementation (only MVP backend)
//! * `pi_proc.rs`      — child-process supervision primitives
//! * `pi_events.rs`    — NDJSON event-stream folding
//! * `pi_extract.rs`   — JSON-block extraction from final prose
// Windows keeps provider and broker contracts compiled while repository execution stays disabled.
#![cfg_attr(windows, allow(dead_code))]

pub mod antigravity_harness;
pub mod claude_harness;
pub mod codex_harness;
pub mod copilot_harness;
pub(crate) mod dependency_authorization;
pub(crate) mod discovery;
pub(crate) mod execution_security;
pub(crate) mod host_tools;
pub mod live_preview;
pub mod opencode_harness;
pub mod pi_events;
pub mod pi_extract;
pub mod pi_harness;
pub mod pi_proc;
pub(crate) mod pi_provider_settings;
pub mod prompt_catalog;
#[cfg(test)]
#[path = "harness/provider_mode_matrix.rs"]
mod provider_mode_matrix;
pub mod responses;
pub mod runtime_capabilities;
pub(crate) mod runtime_config;

pub const CODEX_HARNESS_ENV: &str = "KOOLADE_HARNESS";

pub(crate) fn implementation_route_available(harness: &str) -> bool {
    execution_security::implementation_route_available(harness)
}

/// Entry point used by the desktop binary's private stdio MCP subprocess.
#[doc(hidden)]
pub fn serve_internal_mcp_server(config_path: &std::path::Path) -> anyhow::Result<()> {
    execution_security::serve_mcp_server(config_path)
}

pub(crate) fn require_application_implementation_boundary(
    harness: &str,
    request: &PlanningRequest,
) -> Result<(), crate::error::AppError> {
    validate_review_invocation(request)?;
    #[cfg(test)]
    if provider_test_override::enabled() {
        return Ok(());
    }
    let policy_id = match harness {
        "Claude Code" => "claude",
        "Copilot CLI" => "copilot",
        other => other,
    };
    if let Some(reason) = execution_security::task_execution_block_reason(policy_id) {
        return Err(crate::error::AppError::HarnessFailed {
            reason: format!("{reason} No CLI was started."),
            stderr_tail: String::new(),
        });
    }
    if !execution_security::for_harness(policy_id)
        .is_some_and(|capabilities| capabilities.supports(request.mode))
    {
        return Err(crate::error::AppError::HarnessFailed {
            reason: format!(
                "{harness} repository access is unavailable because it has no configured application-owned Docker execution policy. Select a supported CLI and configure an approved Docker execution target. No CLI was started."
            ),
            stderr_tail: String::new(),
        });
    }
    Ok(())
}

pub(crate) fn validate_review_invocation(
    request: &PlanningRequest,
) -> Result<(), crate::error::AppError> {
    let Some(identity) = &request.review_identity else {
        return Ok(());
    };
    let valid_uuid = |value: &str| {
        uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.hyphenated().to_string() == value)
    };
    let role_phase = request.prompt_manifest.as_ref().and_then(|manifest| {
        match manifest.role_version.as_str() {
            "code_review/v1" | "code_review/v2" => Some("independent_code_review"),
            "plan_reviewer/v1" => Some("independent_plan_review"),
            "runtime_qa/v2" | "runtime_qa/v3" => Some("runtime_qa_review"),
            _ => None,
        }
    });
    let screenshot_review = identity.runtime_qa_screenshot_review.is_some();
    let expected_mode = if screenshot_review {
        api::ExecutionMode::RuntimeQaVisualReview
    } else {
        api::ExecutionMode::ReadOnlyAnalysis
    };
    let valid = request.mode == expected_mode
        && request.execution_workspace.is_none()
        && request.telemetry_phase.as_deref() == role_phase
        && request.prompt_manifest.as_ref().is_some_and(|manifest| {
            matches!(
                manifest.role_version.as_str(),
                "code_review/v1"
                    | "code_review/v2"
                    | "plan_reviewer/v1"
                    | "runtime_qa/v2"
                    | "runtime_qa/v3"
            ) && manifest.privilege_profile == "read_only; isolated_fresh_review_context"
                && (!screenshot_review || manifest.role_version == "runtime_qa/v3")
        })
        && valid_uuid(&identity.execution_id)
        && valid_uuid(&identity.session_id)
        && identity.execution_id != identity.session_id;
    let valid = valid
        && request.prompt_body.contains(&identity.execution_id)
        && request.prompt_body.contains(&identity.session_id);
    if valid {
        Ok(())
    } else {
        Err(crate::error::AppError::HarnessFailed {
            reason: "Independent review request lacks a valid fresh controller run/session identity, reviewer prompt, or read-only boundary. No provider was started.".into(),
            stderr_tail: String::new(),
        })
    }
}

#[cfg(test)]
pub(crate) fn with_uncontained_provider_test_execution<T>(run: impl FnOnce() -> T) -> T {
    provider_test_override::with_enabled(run)
}

#[cfg(test)]
pub(crate) fn uncontained_provider_test_execution_enabled() -> bool {
    provider_test_override::enabled()
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
    let configured = std::path::PathBuf::from(value);
    if !configured.is_absolute() {
        return Err(crate::error::AppError::HarnessNotFound {
            detail: format!("Configured {id} executable path must be absolute"),
        });
    }
    let path = configured
        .canonicalize()
        .map_err(|_| crate::error::AppError::HarnessNotFound {
            detail: format!(
                "Configured {id} executable path does not exist: {}",
                configured.display()
            ),
        })?;
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
pub(crate) use api::normalize_provider_error;
pub(crate) use api::validate_reasoning_level;
pub use api::{
    ActivityTelemetry, AiHarness, ApplicationAction, BootstrapConversationReplyV1,
    BootstrapConversationResponseV1, BootstrapImageProfileV1, BootstrapIntentOperationV1,
    BootstrapIntentSuggestionV1, CleanLaunchTelemetry, DependencyAuthorizationScope,
    DependencyAuthorizationSource, DependencyDecision, DependencyFailureCategory, DependencyKind,
    DependencyNeed, DependencyPackageIdentity, DependencyPreparationStatus,
    DependencyPreparationTelemetry, DependencyRequest, DependencyRequestStatus,
    DependencyRetryResult, DocumentUpdate, ExecutionMode, HarnessExecutionFailure,
    HarnessFailureCategory, HarnessOutcome, HarnessRouteProvenance, LivePost, LiveProgress,
    ModelCallUsage, PackageEcosystem, PlanningRequest, PlanningTaskDraft, PlanningTaskOffer,
    RequestedAction, RetrievalPlan, ReviewExecutionIdentity, RuntimeQaReviewScreenshot,
    RuntimeQaScreenshotReview, ToolAccess, TurnEnvelope, TurnItem, TurnItemUpdate,
};
pub use claude_harness::ClaudeHarness;
pub use codex_harness::CodexHarness;
pub use copilot_harness::CopilotHarness;
pub use opencode_harness::OpenCodeHarness;
pub use pi_harness::PiHarness;

mod api;
mod nuget_audit;
pub(crate) mod resource_bridge;

pub(crate) fn refresh_nuget_audit_cache(timeout: std::time::Duration) -> anyhow::Result<()> {
    nuget_audit::refresh(timeout)
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
