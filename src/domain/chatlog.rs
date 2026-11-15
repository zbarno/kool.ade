//! Chat history record types.
//!
//! The conversation is *persistent runtime state*: it survives restarts, but
//! it is deliberately stored OUTSIDE the git repository, under
//! `~/.koolade-packet/projects/<slug>/chat.jsonl` (storage lives in
//! `crate::persistence::chat_store`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

mod manager;
pub use manager::{
    ManagerEventCheckpointV1, ManagerInputEventV1, ManagerRunEventV1, ManagerRunOutcomeV1,
    checkpoint_attempts,
};

/// Who authored a chat line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Agent,
    /// Local/system notice (errors, harness cancellations, sync notes).
    System,
}

/// One line in the planner conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// Monotonic id derived from wall-clock + process sequence; unique per store.
    pub id: String,
    pub role: ChatRole,
    /// Plain text (never Markdown-rendered from user input).
    pub text: String,
    /// Optional open-item reference the message relates to (e.g. the item a
    /// question targeted or an answer resolved).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_item: Option<String>,
    /// Fingerprint of the task blocker explicitly answered by this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_context: Option<String>,
    /// Marks a clarification-only exchange, which cannot serve as a submitted decision.
    #[serde(default)]
    pub clarification: bool,
    /// Target and published source revision for Specification chat turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specification_context: Option<SpecificationChatContext>,
    /// Correlates a baseline review result with the specific UI request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_review_id: Option<String>,
    /// Bounded, receipt-verified source excerpts from a Manager review.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verified_repository_evidence: Vec<VerifiedRepositoryEvidence>,
    /// Durable lease heartbeat; the visible event timestamp remains unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_renewed_at: Option<DateTime<Utc>>,
    /// Structured Manager supervision checkpoint, stored in private activity history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manager_run_event: Option<ManagerRunEventV1>,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecificationChatContext {
    pub scope_key: String,
    pub intent: String,
    pub published_reference: String,
    pub published_commit_sha: String,
    pub source_documents: Vec<SpecificationSourceDocument>,
    pub effective_document_sha256: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviewed_repository_evidence: Vec<ReviewedRepositoryEvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedRepositoryEvidenceRef {
    pub evidence_id: String,
    pub repository_id: String,
    pub source_ref: String,
    pub source_sha: String,
    pub content_sha256: String,
    #[serde(default)]
    pub excerpt_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedRepositoryEvidence {
    pub evidence_id: String,
    pub repository_id: String,
    pub source_ref: String,
    pub source_sha: String,
    pub content_sha256: String,
    #[serde(default)]
    pub excerpt_sha256: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecificationSourceDocument {
    pub path: String,
    pub sha256: String,
}

impl ChatMessage {
    pub fn new(role: ChatRole, text: impl Into<String>, ref_item: Option<String>) -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let micros = Utc::now().timestamp_micros();
        Self {
            id: format!("{micros:x}-{seq:x}-{:?}", std::process::id()),
            role,
            text: text.into(),
            ref_item,
            answer_context: None,
            clarification: false,
            specification_context: None,
            baseline_review_id: None,
            verified_repository_evidence: Vec::new(),
            lease_renewed_at: None,
            manager_run_event: None,
            ts: Utc::now(),
        }
    }

    /// Seconds-precision UTC stamp for tooltips: `HH:MM:SS`.
    pub fn time_label(&self) -> String {
        self.ts.format("%H:%M:%S").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trips_ref_option() {
        let m = ChatMessage::new(ChatRole::Agent, "hello world", Some("CLR-001".into()));
        let j = serde_json::to_string(&m).expect("encode");
        let back: ChatMessage = serde_json::from_str(&j).expect("decode");
        assert_eq!(back, m);
        let m2 = ChatMessage::new(ChatRole::User, "plain", None);
        let j2 = serde_json::to_string(&m2).expect("encode");
        assert!(!j2.contains("ref_item"));
    }

    #[test]
    fn ids_are_unique_within_process() {
        let a = ChatMessage::new(ChatRole::User, "a", None);
        let b = ChatMessage::new(ChatRole::User, "b", None);
        assert_ne!(a.id, b.id);
    }
}
