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

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

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
    /// Pi thinking level selected by the Koolade role that owns this turn.
    pub reasoning_level: String,
    /// Optional backend model selected by application routing or task policy.
    pub model: Option<String>,
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

/// Model-selected logical sources for a bounded planning turn. References are
/// suggestions only; the application resolves each against its current catalog.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RetrievalPlan {
    pub documents: Vec<String>,
    pub open_items: Vec<String>,
    pub repository_areas: Vec<String>,
}

/// Display snapshot; task snapshots are persisted privately, never as planning artifacts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveProgress {
    pub telemetry: ActivityTelemetry,
    /// Zero-based acceptance-criterion indexes reported complete by the task worker.
    /// This is a full snapshot and is persisted with the task activity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checklist: Vec<usize>,
    #[serde(default)]
    pub checklist_revision: u64,
    pub posts: Vec<LivePost>,
    pub thoughts: String,
    pub response: String,
    pub specification: Option<String>,
    pub activity: Option<String>,
}

/// Observed stream updates, not estimated token counts. Persisted with task activity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActivityTelemetry {
    pub started_ms: Option<i64>,
    pub updated_ms: Option<i64>,
    pub finished_ms: Option<i64>,
    pub updates: u64,
    /// Ten-second buckets: UTC bucket number and received update count.
    pub samples: Vec<(i64, u64)>,
    /// Usage fields reported by a harness, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// A stable, chronologically placed block of external agent output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LivePost {
    pub id: (u64, usize),
    pub kind: String,
    pub text: String,
}

impl LiveProgress {
    pub fn update(&mut self, mut next: Self) {
        // Progress snapshots can be delayed behind a newer snapshot. Checklist
        // reports carry a worker-local revision so late snapshots cannot roll
        // the durable board state back.
        if next.checklist_revision < self.checklist_revision {
            next.checklist = std::mem::take(&mut self.checklist);
            next.checklist_revision = self.checklist_revision;
        }
        for post in next.posts.drain(..) {
            if let Some(existing) = self.posts.iter_mut().find(|p| p.id == post.id) {
                *existing = post;
            } else {
                self.posts.push(post);
            }
        }
        next.posts = std::mem::take(&mut self.posts);
        let now = chrono::Utc::now().timestamp_millis();
        let mut telemetry = std::mem::take(&mut self.telemetry);
        telemetry.started_ms.get_or_insert(now);
        telemetry.updated_ms = Some(now);
        telemetry.updates += 1;
        telemetry.input_tokens = next.telemetry.input_tokens.or(telemetry.input_tokens);
        telemetry.output_tokens = next.telemetry.output_tokens.or(telemetry.output_tokens);
        telemetry.model = next.telemetry.model.take().or(telemetry.model);
        let bucket = now / 10_000;
        if let Some((_, count)) = telemetry
            .samples
            .last_mut()
            .filter(|(last, _)| *last == bucket)
        {
            *count += 1;
        } else {
            telemetry.samples.push((bucket, 1));
        }
        telemetry.samples.retain(|(time, _)| *time >= bucket - 59);
        next.telemetry = telemetry;
        *self = next;
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentUpdate {
    #[serde(alias = "document_id")]
    pub document_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<crate::domain::ChangeStatus>,
}

/// A follow-up task discovered during a documentation refresh. Findings stay
/// advisory until a person triages them; this record never requests a code fix.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanningTaskDraft {
    pub title: String,
    pub description: String,
    pub kind: crate::core::planning_work::WorkKind,
    pub status: crate::core::planning_work::WorkStatus,
}

/// Normalized internal planning projection. Raw operation responses are
/// decoded through `harness::responses` before reaching this model.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnEnvelope {
    #[serde(alias = "schema_version")]
    pub schema_version: Option<u32>,
    #[serde(alias = "assistant_message")]
    pub assistant_message: Option<String>,
    /// Short phrase describing what changed (feeds the git commit subject).
    #[serde(alias = "change_summary")]
    pub change_summary: Option<String>,
    /// Full replacements of logical product or feature documents.
    #[serde(alias = "document_updates")]
    pub document_updates: Option<Vec<DocumentUpdate>>,
    #[serde(
        default,
        alias = "planning_tasks",
        skip_serializing_if = "Option::is_none"
    )]
    pub planning_tasks: Option<Vec<PlanningTaskDraft>>,
    /// Complete replacement specification markdown (null when unchanged).
    #[serde(alias = "updated_specification")]
    pub updated_specification: Option<String>,
    #[serde(alias = "open_items_added")]
    pub open_items_added: Option<Vec<TurnItem>>,
    #[serde(alias = "open_items_updated")]
    pub open_items_updated: Option<Vec<TurnItemUpdate>>,
    #[serde(alias = "open_items_resolved")]
    pub open_items_resolved: Option<Vec<String>>,
    /// The item the agent wants to ask NOW (must satisfy routing rules).
    #[serde(alias = "next_question_id")]
    pub next_question_id: Option<String>,
    /// A typed interpretation of an explicit user request. Rust rechecks the
    /// action against the current workflow before dispatching it.
    #[serde(default, alias = "requested_action")]
    pub requested_action: Option<RequestedAction>,
    #[serde(default, alias = "follow_up_task")]
    pub follow_up_task: Option<PlanningTaskOffer>,
    pub interview: Option<crate::core::workflow::InterviewBrief>,
    #[serde(alias = "task_stories", skip_serializing_if = "Option::is_none")]
    pub task_stories: Option<Vec<crate::core::workflow::TaskStory>>,
    #[serde(alias = "task_outline", skip_serializing_if = "Option::is_none")]
    pub task_outline: Option<Vec<crate::core::workflow::TaskOutline>>,
    /// Koolade-authored alternatives and advisory lean for Compare Plans.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plans: Option<Vec<crate::domain::PlanAlternative>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<crate::domain::PlanRecommendation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationAction {
    ApproveChange,
    GenerateTasks,
    StartImplementation,
    PauseImplementation,
    ResumeImplementation,
    Publish,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestedAction {
    pub action: ApplicationAction,
    /// Stable Koolade identity for a specifically named change or task.
    #[serde(default, alias = "target_uid")]
    pub target_uid: Option<String>,
}

/// Optional user-consented follow-up offered after a focused Question task.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanningTaskOffer {
    pub title: String,
    pub description: String,
}

impl TurnEnvelope {
    /// Lenient accessors: the contract asks for `[]`, but an agent that omits
    /// an empty list should not tank the whole turn.
    pub fn added(&self) -> &[TurnItem] {
        self.open_items_added.as_deref().unwrap_or_default()
    }
    pub fn updated(&self) -> &[TurnItemUpdate] {
        self.open_items_updated.as_deref().unwrap_or_default()
    }
    pub fn resolved(&self) -> &[String] {
        self.open_items_resolved.as_deref().unwrap_or_default()
    }
    pub fn assistant(&self) -> &str {
        self.assistant_message.as_deref().unwrap_or("")
    }
    pub fn updated_spec(&self) -> Option<&str> {
        self.updated_specification.as_deref()
    }
}

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

/// The boundary every AI backend implements.
pub trait AiHarness: Send + Sync {
    /// Human-readable backend label for the UI (e.g. "pi 0.84.4").
    fn label(&self) -> String;

    /// Availability probe (executables present, sane version).
    fn check_available(&self) -> Result<String, AppError>;

    /// Run one planning turn to completion (spawns/manages the process).
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError>;

    /// Select from Koolade's bounded source catalog before a main planning turn.
    /// Backends without a dedicated retrieval pass use Koolade's safe core context.
    fn plan_retrieval(
        &self,
        _request: &PlanningRequest,
    ) -> Result<Option<RetrievalPlan>, AppError> {
        Ok(None)
    }
}
