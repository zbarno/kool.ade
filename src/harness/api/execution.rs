use std::time::Duration;

use crate::harness::api::LiveProgress;

/// A harness operation's fixed authority and tool capability class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Planning,
    ReadOnlyAnalysis,
    TaskGeneration,
    Investigation,
    Implementation,
    Reconciliation,
    DecisionExplanation,
}

impl ExecutionMode {
    pub const ALL: [Self; 7] = [
        Self::Planning,
        Self::ReadOnlyAnalysis,
        Self::TaskGeneration,
        Self::Investigation,
        Self::Implementation,
        Self::Reconciliation,
        Self::DecisionExplanation,
    ];

    pub fn tool_access(self) -> ToolAccess {
        match self {
            Self::Planning | Self::TaskGeneration | Self::Investigation => ToolAccess::ReadOnly,
            Self::Implementation => ToolAccess::BoundedImplementation,
            Self::ReadOnlyAnalysis | Self::Reconciliation | Self::DecisionExplanation => {
                ToolAccess::None
            }
        }
    }
}

/// Tool authority is derived from an operation mode instead of independently
/// configurable booleans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAccess {
    None,
    ReadOnly,
    BoundedImplementation,
}

/** One planning turn submitted to the external harness.
/// Transport-neutral: the app supplies *context it owns*; the harness
/// renders it in whatever shape the backing CLI prefers. */
#[derive(Debug, Clone)]
pub struct PlanningRequest {
    /// The only source of execution capabilities for this request.
    pub mode: ExecutionMode,
    /// Stable Koolade task identity supplied by the application, never the worker.
    pub task_id: Option<String>,
    /// Pi thinking level selected by the Koolade role that owns this turn.
    pub reasoning_level: String,
    /// Implementation subphase for telemetry attribution, when applicable.
    pub telemetry_phase: Option<String>,
    /// Repository working directory the harness process must run in (§18).
    pub repo_root: std::path::PathBuf,
    /// Fully rendered prompt body (system instructions travel separately).
    pub prompt_body: String,
    /// Planner system-persona instructions (appended to the harness defaults).
    pub system_instructions: String,
    /// Wall-clock budget for the whole turn.
    pub timeout: Duration,
    /// Sink for transient thinking, response, document, and activity snapshots.
    pub progress_tx: std::sync::mpsc::Sender<LiveProgress>,
    /// Cooperative cancellation set by the UI's Cancel button.
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
