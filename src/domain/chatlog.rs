//! Chat history record types.
//!
//! The conversation is *persistent runtime state*: it survives restarts, but
//! it is deliberately stored OUTSIDE the git repository, under
//! `~/.koolade-packet/projects/<slug>/chat.jsonl` (storage lives in
//! `crate::persistence::chat_store`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
    pub ts: DateTime<Utc>,
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
