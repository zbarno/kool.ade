//! The external-AI-harness boundary (SPECIFICATION.md §14–§15).
//!
//! Named-file module layout:
//! * `api.rs`          — trait, request/outcome/envelope types
//! * `pi_harness.rs`   — the Pi CLI implementation (only MVP backend)
//! * `pi_proc.rs`      — child-process supervision primitives
//! * `pi_events.rs`    — NDJSON event-stream folding
//! * `pi_extract.rs`   — JSON-block extraction from final prose

pub mod live_preview;
pub mod pi_events;
pub mod pi_extract;
pub mod pi_harness;
pub mod pi_proc;
pub(crate) mod pi_sandbox;
pub mod responses;
pub mod runtime_capabilities;

pub use api::{
    ActivityTelemetry, AiHarness, ApplicationAction, DocumentUpdate, ExecutionMode, HarnessOutcome,
    LivePost, LiveProgress, ModelCallUsage, PlanningRequest, PlanningTaskDraft, PlanningTaskOffer,
    RequestedAction, RetrievalPlan, ToolAccess, TurnEnvelope, TurnItem, TurnItemUpdate,
};
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

pub(crate) fn prepared_npm_cache_path() -> anyhow::Result<std::path::PathBuf> {
    resource_bridge::prepared_npm_cache_path()
}

pub(crate) fn prepared_npm_cache_covers(
    worktree: &std::path::Path,
    cache: &std::path::Path,
) -> bool {
    resource_bridge::prepared_npm_cache_covers(worktree, cache)
}
