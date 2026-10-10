//! One unresolved planning issue with optional decision support.
use super::{Authority, ItemKind, ItemStatus, Priority};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenItem {
    /// Immutable Koolade identity, absent only on artifacts awaiting migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    /// Stable identifier, e.g. `CLR-012`. Allocated by the app, referenced thereafter.
    pub id: String,
    /// Stable origin for a generated board item before it receives a CLR number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub priority: Priority,
    #[serde(default)]
    pub authority: Authority,
    pub kind: ItemKind,
    /// Routing category, e.g. `Security`, `Product`, `General`.
    pub category: String,
    /// Person or group responsible for answering (None → nobody assigned).
    pub assigned_to: Option<String>,
    /// The actual question/issue, in one or two sentences.
    pub question: String,
    /// Why this matters / how it was discovered (context for the responder).
    pub reason: String,
    #[serde(default)]
    pub feature_id: Option<String>,
    /// Stable feature relationship, populated when the feature has a Koolade UID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_uid: Option<String>,
    #[serde(default)]
    pub recommendation: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_brief: Option<crate::domain::DecisionBrief>,
    /// Open board-item IDs that must resolve before this item is actionable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
    pub status: ItemStatus,
    /// Revision captured when this item was loaded; omitted from legacy data.
    #[serde(skip)]
    pub record_revision: u64,
    /// Serialized content captured at load time, used to distinguish this
    /// caller's edits from unrelated changes made by another planner.
    #[serde(skip)]
    #[doc(hidden)]
    pub record_baseline: Option<String>,
}

impl PartialEq for OpenItem {
    fn eq(&self, other: &Self) -> bool {
        self.uid == other.uid
            && self.id == other.id
            && self.conversation_id == other.conversation_id
            && self.priority == other.priority
            && self.authority == other.authority
            && self.kind == other.kind
            && self.category == other.category
            && self.assigned_to == other.assigned_to
            && self.question == other.question
            && self.reason == other.reason
            && self.feature_id == other.feature_id
            && self.feature_uid == other.feature_uid
            && self.recommendation == other.recommendation
            && self.evidence == other.evidence
            && self.decision_brief == other.decision_brief
            && self.blocked_by == other.blocked_by
            && self.status == other.status
    }
}

impl Eq for OpenItem {}

impl OpenItem {
    pub fn new(
        id: String,
        priority: Priority,
        kind: ItemKind,
        category: String,
        assigned_to: Option<String>,
        question: String,
        reason: String,
    ) -> Self {
        Self {
            uid: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
            id,
            conversation_id: None,
            priority,
            authority: Authority::Human,
            kind,
            category,
            assigned_to,
            question,
            reason,
            feature_id: None,
            feature_uid: None,
            recommendation: String::new(),
            evidence: String::new(),
            decision_brief: None,
            blocked_by: Vec::new(),
            status: ItemStatus::Open,
            record_revision: 0,
            record_baseline: None,
        }
    }

    pub fn conversation_key(&self) -> &str {
        self.conversation_id.as_deref().unwrap_or(&self.id)
    }

    pub const fn is_ownership_gap(&self) -> bool {
        matches!(self.kind, ItemKind::Ownership)
    }

    pub fn summary(&self) -> String {
        let question = collapse(&self.question, 90);
        format!("{} · {question}", self.id)
    }
}

fn collapse(text: &str, max_chars: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    let mut out = flat
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}
