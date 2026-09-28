//! One unresolved planning issue with optional decision support.
use super::{Authority, ItemKind, ItemStatus, Priority};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenItem {
    /// Immutable Packet identity, absent only on artifacts awaiting migration.
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
    /// Stable feature relationship, populated when the feature has a Packet UID.
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
}

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
