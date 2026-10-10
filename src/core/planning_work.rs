//! Versioned durable work records for planning requests that may not yet have
//! produced a feature specification.
use serde::{Deserialize, Serialize};

pub const FILE: &str = crate::artifacts::layout::canonical::WORK;
pub const STORE_FILE: &str = crate::artifacts::planning_store::paths::WORK;
const SCHEMA_VERSION: u32 = 3;

mod projection;
mod storage;
mod validation;
use validation::validate;
pub use validation::{routing_for_feature, save, save_expected};
mod reconcile;
#[cfg(test)]
mod tests;

pub use projection::{cards, context, link_feature_identities};
pub use reconcile::{completed_turn_status, reconcile_inactive, reconcile_inactive_excluding};
pub use storage::{append_discovered, find, load, load_expected};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    #[default]
    Feature,
    Bug,
    NewProject,
    DocumentationRefresh,
    Question,
    TaskGeneration,
}

impl WorkKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Feature => "Feature",
            Self::Bug => "Bug",
            Self::NewProject => "New Project",
            Self::DocumentationRefresh => "Refresh Documentation",
            Self::Question => "Question",
            Self::TaskGeneration => "Task generation",
        }
    }

    pub fn planning_guidance(self) -> &'static str {
        match self {
            Self::Feature => {
                "Plan the requested new behavior and create a concise feature specification. Ground it in relevant existing documents and avoid boilerplate or repeating details in multiple sections."
            }
            Self::Bug => {
                "Inspect current behavior and repository evidence in bug-triage mode: first reproduce the reported failure or trace the relevant execution path, then identify and record the most likely root cause with exact repository evidence before planning a fix. Separate observed facts from hypotheses, and state what evidence would confirm or rule out each hypothesis. Inspect only the relevant repository slice and continue in bounded chunks; do not attempt a whole-repository survey. Once the cause is supported, write a concise corrective specification with the smallest fix, affected behavior, regression checks, and any risks. Do not plan a speculative fix while the cause is unknown. Do not ask about already established intended behavior unless expected behavior is genuinely unclear; ask only for missing reproduction details or evidence the user uniquely has. If evidence cannot be obtained safely, record the precise blocker and next diagnostic step rather than guessing. Avoid duplicating the report or repeating a requirement across sections."
            }
            Self::NewProject => {
                "Establish the product problem, users, outcome, scope, constraints, major behaviors, architecture, risks, and unknowns progressively. Record only agreed or evidenced details; do not fill unknown sections with generic boilerplate or repeat the same fact."
            }
            Self::DocumentationRefresh => {
                "Analyze the existing repository to build or refresh its project documentation. Start from existing repo-level documentation and inspect source code to verify and fill gaps. If Kool.ad/e project artifacts are missing or incomplete, create useful evidence-based product documents instead of assuming an existing specification is complete. Record contradictions, missing decisions, and unresolved questions as Needs Attention tasks. Record suspected bugs, vulnerabilities, and other quality risks as Todo triage tasks with evidence and uncertainty clearly stated. Do not fix findings or propose implementation as if confirmed. Preserve established facts, avoid boilerplate, and continue in bounded coherent slices until the codebase has been adequately documented."
            }
            Self::Question => {
                "Investigate and answer directly with repository evidence. Do not create a specification unless the answer uncovers a separate user decision."
            }
            Self::TaskGeneration => {
                "Generate implementation task stories from the current explicitly approved feature specification."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    #[default]
    Todo,
    InProgress,
    InReview,
    NeedsAttention,
    Done,
}

impl WorkStatus {
    /// Board layout projection; status remains the persisted workflow state.
    pub fn board_column(self) -> usize {
        match self {
            Self::Todo => 0,
            Self::InProgress => 1,
            Self::InReview => 2,
            Self::NeedsAttention => 3,
            Self::Done => 4,
        }
    }

    fn from_legacy_column(column: usize) -> anyhow::Result<Self> {
        match column {
            0 => Ok(Self::Todo),
            1 => Ok(Self::InProgress),
            2 => Ok(Self::InReview),
            3 => Ok(Self::NeedsAttention),
            4 => Ok(Self::Done),
            _ => anyhow::bail!("Unsupported legacy planning work column {column}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Work {
    pub uid: String,
    pub key: String,
    pub kind: WorkKind,
    pub title: String,
    pub request: String,
    pub status: WorkStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_branch: Option<String>,
    /// Explicit task-owned routes; missing categories remain application defaults.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub routing_overrides:
        std::collections::BTreeMap<String, crate::persistence::harness_settings::WorkRoute>,
    /// UID of the related task whose route overrides were copied here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_inherited_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_up_task: Option<FollowUpTaskOffer>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FollowUpTaskOffer {
    pub title: String,
    pub description: String,
}

impl Work {
    pub fn new(key: String, title: String, request: String, detail: String) -> Self {
        Self {
            uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            key,
            kind: WorkKind::Feature,
            title,
            request,
            status: WorkStatus::InProgress,
            feature_id: None,
            feature_uid: None,
            parent_uid: None,
            source_branch: None,
            destination_branch: None,
            routing_overrides: std::collections::BTreeMap::new(),
            routing_inherited_from: None,
            follow_up_task: None,
            detail,
        }
    }

    pub fn board_column(&self) -> usize {
        self.status.board_column()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkFile {
    schema_version: u32,
    items: Vec<Work>,
}
