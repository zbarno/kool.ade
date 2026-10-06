//! Versioned durable work records for planning requests that may not yet have
//! produced a feature specification.
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILE: &str = crate::artifacts::layout::canonical::WORK;
const SCHEMA_VERSION: u32 = 2;

mod projection;
mod reconcile;
#[cfg(test)]
mod tests;

pub use projection::{cards, context, link_feature_identities};
pub use reconcile::{completed_turn_status, reconcile_inactive, reconcile_inactive_excluding};

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWork {
    key: String,
    title: String,
    request: String,
    column: usize,
    feature: Option<String>,
    detail: String,
}

pub fn load(repo: &Path) -> anyhow::Result<Vec<Work>> {
    let path = repo.join(FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if value.is_array() {
        let legacy: Vec<LegacyWork> = serde_json::from_value(value)?;
        let work: Vec<Work> = legacy
            .into_iter()
            .map(migrate_legacy)
            .collect::<anyhow::Result<_>>()?;
        save(repo, &work)?;
        return Ok(work);
    }
    let file: WorkFile = serde_json::from_value(value)?;
    anyhow::ensure!(
        (1..=SCHEMA_VERSION).contains(&file.schema_version),
        "Unsupported planning work schema version {}",
        file.schema_version
    );
    validate(&file.items)?;
    if file.schema_version < SCHEMA_VERSION {
        save(repo, &file.items)?;
    }
    Ok(file.items)
}

fn migrate_legacy(work: LegacyWork) -> anyhow::Result<Work> {
    Ok(Work {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        key: work.key,
        kind: WorkKind::Feature,
        title: work.title,
        request: work.request,
        status: WorkStatus::from_legacy_column(work.column)?,
        feature_id: work.feature,
        feature_uid: None,
        parent_uid: None,
        follow_up_task: None,
        detail: work.detail,
    })
}

fn validate(work: &[Work]) -> anyhow::Result<()> {
    let mut uids = std::collections::BTreeSet::new();
    for item in work {
        anyhow::ensure!(
            uuid::Uuid::parse_str(&item.uid).is_ok(),
            "Planning work has invalid UID: {}",
            item.uid
        );
        anyhow::ensure!(
            uids.insert(item.uid.as_str()),
            "Duplicate planning work UID"
        );
        if let Some(uid) = &item.parent_uid {
            anyhow::ensure!(
                uuid::Uuid::parse_str(uid).is_ok(),
                "Invalid parent work UID"
            );
        }
        if let Some(uid) = &item.feature_uid {
            anyhow::ensure!(uuid::Uuid::parse_str(uid).is_ok(), "Invalid feature UID");
        }
    }
    Ok(())
}

pub fn save(repo: &Path, work: &[Work]) -> anyhow::Result<()> {
    validate(work)?;
    crate::artifacts::task_docs::safe_directory(repo, crate::artifacts::layout::canonical::STATE)?;
    let path = repo.join(FILE);
    anyhow::ensure!(
        !std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()),
        "Linked planning work ledger"
    );
    let file = WorkFile {
        schema_version: SCHEMA_VERSION,
        items: work.to_vec(),
    };
    crate::artifacts::atomic_write(&path, &serde_json::to_string_pretty(&file)?)
}

/// Build the versioned work ledger content for validated discovery tasks so
/// the caller can include it in the same artifact transaction as documents.
pub fn append_discovered(
    repo: &Path,
    drafts: &[crate::harness::PlanningTaskDraft],
) -> anyhow::Result<Option<String>> {
    if drafts.is_empty() {
        return Ok(None);
    }
    let mut items = load(repo)?;
    for draft in drafts {
        let mut work = Work::new(
            String::new(),
            draft.title.trim().to_owned(),
            draft.description.trim().to_owned(),
            "Discovered during repository documentation review; triage before acting.".into(),
        );
        work.key = format!("task:{}", work.uid);
        work.kind = draft.kind;
        work.status = draft.status;
        items.push(work);
    }
    validate(&items)?;
    Ok(Some(serde_json::to_string_pretty(&WorkFile {
        schema_version: SCHEMA_VERSION,
        items,
    })?))
}

pub fn find(state: &crate::core::state::PlannerState, key: &str) -> Option<Work> {
    load(&state.repo_root)
        .ok()?
        .into_iter()
        .find(|item| item.key == key)
}
