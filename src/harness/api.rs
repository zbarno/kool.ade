//! Narrow harness boundary (SPECIFICATION.md §15).
//!
//! ```text
//! AiHarness
//!   └── PiHarness      (MVP implementation; see `pi_harness`)
//! ```
//!
//! The rest of the application only sees [`AiHarness`]: it hands over a
//! [`PlanningRequest`] and receives either a [`HarnessOutcome`] or a typed
//! [`crate::AppError`]. Nothing Pi-specific leaks upward. Future harnesses
//! (Codex CLI, Copilot CLI, Claude Code…) plug in by implementing the trait.

use serde::{Deserialize, Serialize};

use crate::error::AppError;

mod activity;
mod dependency;
mod execution;
mod execution_failure;
mod planning;
mod reasoning;
mod route;
pub use activity::{
    ActivityTelemetry, CleanLaunchTelemetry, LivePost, LiveProgress, ModelCallUsage,
};
pub use dependency::{
    DependencyAuthorizationScope, DependencyAuthorizationSource, DependencyDecision,
    DependencyFailureCategory, DependencyKind, DependencyNeed, DependencyPackageIdentity,
    DependencyPreparationStatus, DependencyPreparationTelemetry, DependencyRequest,
    DependencyRequestStatus, DependencyRetryResult, PackageEcosystem,
};
pub use execution::{
    ExecutionMode, PlanningRequest, RuntimeQaReviewScreenshot, RuntimeQaScreenshotReview,
    ToolAccess,
};
pub(crate) use execution_failure::normalize_provider_error;
pub use execution_failure::{HarnessExecutionFailure, HarnessFailureCategory};
pub use planning::*;
pub(crate) use reasoning::validate_reasoning_level;
pub use route::HarnessRouteProvenance;

/// A brand-new open item proposed by the agent.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnItem {
    #[serde(default)]
    pub id: Option<String>,
    pub priority: Option<String>,
    pub authority: Option<String>,
    #[serde(alias = "itemType", alias = "item_type")]
    pub kind: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    #[serde(alias = "assigned_to")]
    pub assigned_to: Option<String>,
    pub question: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, alias = "feature_id")]
    pub feature_id: Option<String>,
    #[serde(default)]
    pub recommendation: Option<String>,
    #[serde(default)]
    pub evidence: Option<String>,
    #[serde(default)]
    #[serde(alias = "decision_brief")]
    pub decision_brief: Option<crate::domain::DecisionBrief>,
    #[serde(default, alias = "blocked_by")]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    #[serde(alias = "resolution_note")]
    pub resolution_note: Option<String>,
}

/// Partial update of an existing item (identified by `id`).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnItemUpdate {
    pub id: Option<String>,
    pub priority: Option<String>,
    pub authority: Option<String>,
    #[serde(alias = "itemType", alias = "item_type")]
    pub kind: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    #[serde(alias = "assigned_to")]
    pub assigned_to: Option<String>,
    pub question: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, alias = "feature_id")]
    pub feature_id: Option<String>,
    #[serde(default)]
    pub recommendation: Option<String>,
    #[serde(default)]
    pub evidence: Option<String>,
    #[serde(default)]
    #[serde(alias = "decision_brief")]
    pub decision_brief: Option<crate::domain::DecisionBrief>,
    #[serde(default, alias = "blocked_by")]
    pub blocked_by: Option<Vec<String>>,
}

/// Successful harness termination.
#[derive(Debug, Clone)]
pub struct HarnessOutcome {
    /// Authoritative final assistant text (prose + JSON block).
    pub final_text: String,
    /// Decoded envelope when the final text carried a valid-looking JSON
    /// block. `None` ⇒ no block found or unparseable (fatal downstream).
    pub envelope: Option<TurnEnvelope>,
    /// Last few stderr lines, kept for diagnostics only.
    pub stderr_tail: String,
}

/// Controller-owned identity for one independent reviewer invocation.
/// It is created at dispatch and is also written to local invocation telemetry.
#[derive(Debug, Clone)]
pub struct ReviewExecutionIdentity {
    pub execution_id: String,
    pub session_id: String,
    /// Present only for an independent Runtime QA visual review. The tool
    /// authority is derived from the review mode and this app-owned payload.
    pub runtime_qa_screenshot_review: Option<std::sync::Arc<RuntimeQaScreenshotReview>>,
}

impl PartialEq for ReviewExecutionIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.execution_id == other.execution_id && self.session_id == other.session_id
    }
}

impl Eq for ReviewExecutionIdentity {}

/// The boundary every AI backend implements.
pub trait AiHarness: Send + Sync {
    /// Human-readable backend label for the UI (e.g. "pi 0.84.4").
    fn label(&self) -> String;

    /// Route identity and capability evidence captured when this adapter was
    /// selected. Wrapper harnesses preserve this for durable invocation logs.
    fn route_provenance(&self, _request: &PlanningRequest) -> Option<HarnessRouteProvenance> {
        None
    }

    /// Availability probe (executables present, sane version).
    fn check_available(&self) -> Result<String, AppError>;

    /// Run one planning turn to completion (spawns/manages the process).
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError>;

    /// Run an independent review under a fresh controller-owned session.
    /// The identity is copied into the request so concrete providers can
    /// validate the CodeReviewer role and fresh-process boundary before spawn.
    fn execute_review(
        &self,
        request: &PlanningRequest,
        identity: &ReviewExecutionIdentity,
    ) -> Result<HarnessOutcome, AppError> {
        let mut review_request = request.clone();
        review_request.review_identity = Some(identity.clone());
        self.execute(&review_request)
    }

    /// Stable author identity for the currently active controller-owned run.
    /// Implementations that do not have one cannot authorize independent review.
    fn current_execution_identity(&self) -> Option<ReviewExecutionIdentity> {
        None
    }

    /// Run with an optional request-scoped model choice. Adapters that do not
    /// expose model selection keep their established CLI default.
    fn execute_with_model(
        &self,
        request: &PlanningRequest,
        _model: Option<&str>,
    ) -> Result<HarnessOutcome, AppError> {
        self.execute(request)
    }

    /// Select from Koolade's bounded source catalog before a main planning turn.
    /// Backends without a dedicated retrieval pass use Koolade's safe core context.
    fn plan_retrieval(
        &self,
        _request: &PlanningRequest,
    ) -> Result<Option<RetrievalPlan>, AppError> {
        Ok(None)
    }
}
