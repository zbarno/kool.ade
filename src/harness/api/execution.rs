use std::time::Duration;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use crate::harness::api::LiveProgress;
use serde::{Deserialize, Serialize};

/// A harness operation's fixed authority and tool capability class.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Planning,
    ReadOnlyAnalysis,
    TaskGeneration,
    Investigation,
    Implementation,
    Reconciliation,
    DecisionExplanation,
    RuntimeQaVisualReview,
}

impl ExecutionMode {
    pub const ALL: [Self; 8] = [
        Self::Planning,
        Self::ReadOnlyAnalysis,
        Self::TaskGeneration,
        Self::Investigation,
        Self::Implementation,
        Self::Reconciliation,
        Self::DecisionExplanation,
        Self::RuntimeQaVisualReview,
    ];

    pub fn tool_access(self) -> ToolAccess {
        match self {
            Self::Planning | Self::TaskGeneration | Self::Investigation => ToolAccess::ReadOnly,
            Self::Implementation => ToolAccess::BoundedImplementation,
            Self::RuntimeQaVisualReview => ToolAccess::RuntimeQaScreenshots,
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
    /// Inspect only validated screenshots supplied by the app for one QA run.
    RuntimeQaScreenshots,
}

/// A screenshot that may be shown to an independent runtime QA reviewer.
#[derive(Clone)]
pub struct RuntimeQaReviewScreenshot {
    pub id: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

/// App-owned images and audit sink for one read-only visual QA review.
#[derive(Clone)]
pub struct RuntimeQaScreenshotReview {
    pub workspace_id: String,
    pub source_sha: String,
    pub screenshots: Vec<RuntimeQaReviewScreenshot>,
    pub inspected_screenshot_ids: Arc<Mutex<BTreeSet<String>>>,
}

impl std::fmt::Debug for RuntimeQaScreenshotReview {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeQaScreenshotReview")
            .field("source_sha", &self.source_sha)
            .field("screenshot_count", &self.screenshots.len())
            .field(
                "image_bytes",
                &self
                    .screenshots
                    .iter()
                    .map(|item| item.bytes.len())
                    .sum::<usize>(),
            )
            .finish_non_exhaustive()
    }
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
    /// Stable planning workspace and immutable source identity selected by the
    /// application for Docker-backed repository tools.
    pub execution_workspace: Option<crate::execution::contract::WorkspaceRef>,
    /// Selected destination branch head pinned by the application before an
    /// implementation tool session is authorized.
    pub destination_sha: Option<String>,
    /// Optional app-owned claim fence for implementation tool sessions.
    pub authorization_verifier:
        Option<std::sync::Arc<dyn crate::execution::session::AuthorizationGenerationVerifier>>,
    /// Versioned identity of the application-owned role prompt used for this turn.
    pub prompt_manifest: Option<crate::harness::prompt_catalog::PromptManifest>,
    /// Pi thinking level selected by the Koolade role that owns this turn.
    pub reasoning_level: String,
    /// Implementation subphase for telemetry attribution, when applicable.
    pub telemetry_phase: Option<String>,
    /// Repository working directory the harness process must run in (§18).
    pub repo_root: std::path::PathBuf,
    /// User checkout that owns local runtime configuration grants for this task clone.
    pub runtime_config_source: Option<std::path::PathBuf>,
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
    /// Controller identity for an independent review invocation. Review
    /// backends must consume this on a newly spawned, non-resumed process.
    pub review_identity: Option<super::ReviewExecutionIdentity>,
}
