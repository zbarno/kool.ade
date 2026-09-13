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

/** One planning turn submitted to the external harness.
/// Transport-neutral: the app supplies *context it owns*; the harness
/// renders it in whatever shape the backing CLI prefers. */
#[derive(Debug, Clone)]
pub struct PlanningRequest {
    /// Load repository instructions for implementation; planning remains isolated.
    pub implementation: bool,
    /// Disable tools for independent project-manager status updates.
    pub read_only: bool,
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

/// Display snapshot; task snapshots are persisted privately, never as planning artifacts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveProgress {
    pub telemetry: ActivityTelemetry,
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

/// Parsed-but-not-yet-validated turn envelope emitted by the agent.
/// Looser than the app's strict domain model: validation lives in
/// `crate::core::validation` and is what guards the file system.
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
    pub interview: Option<crate::core::workflow::InterviewBrief>,
    #[serde(alias = "task_stories")]
    pub task_stories: Option<Vec<crate::core::workflow::TaskStory>>,
    #[serde(alias = "task_outline")]
    pub task_outline: Option<Vec<crate::core::workflow::TaskOutline>>,
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
    #[serde(alias = "itemType", alias = "item_type")]
    pub kind: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    #[serde(alias = "assigned_to")]
    pub assigned_to: Option<String>,
    pub question: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
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
    #[serde(alias = "itemType", alias = "item_type")]
    pub kind: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    #[serde(alias = "assigned_to")]
    pub assigned_to: Option<String>,
    pub question: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
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
}
