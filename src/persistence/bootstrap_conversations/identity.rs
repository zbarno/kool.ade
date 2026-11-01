use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::{ChatMessage, ChatRole};

pub(super) const SCHEMA_VERSION: u8 = 1;
pub(super) const MAX_MESSAGES: usize = 100;
pub(super) const MAX_MESSAGE_BYTES: usize = 16 * 1024;
pub(super) const MAX_HISTORY_BYTES: usize = 128 * 1024;
pub(super) const MAX_STORE_BYTES: usize = 256 * 1024;

/// Durable identity for one machine-local setup conversation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BootstrapConversationIdentityV1 {
    pub task_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct ConversationRecordV1 {
    pub schema_version: u8,
    pub identity: BootstrapConversationIdentityV1,
    pub messages: Vec<StoredMessageV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct StoredMessageV1 {
    id: String,
    role: ChatRole,
    text: String,
    ts: DateTime<Utc>,
}

impl ConversationRecordV1 {
    pub(super) fn new(task_id: &str, messages: &[ChatMessage]) -> anyhow::Result<Self> {
        validate_task_id(task_id)?;
        let messages = messages
            .iter()
            .map(StoredMessageV1::from_message)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let record = Self {
            schema_version: SCHEMA_VERSION,
            identity: BootstrapConversationIdentityV1 {
                task_id: task_id.to_owned(),
            },
            messages,
        };
        record.validate_for(task_id)?;
        Ok(record)
    }

    pub(super) fn validate_for(&self, task_id: &str) -> anyhow::Result<()> {
        validate_task_id(task_id)?;
        anyhow::ensure!(
            self.schema_version == SCHEMA_VERSION,
            "unsupported bootstrap conversation schema"
        );
        anyhow::ensure!(
            self.identity.task_id == task_id,
            "bootstrap conversation identity does not match its setup task"
        );
        anyhow::ensure!(
            self.messages.len() <= MAX_MESSAGES,
            "bootstrap conversation has too many messages"
        );
        anyhow::ensure!(
            self.messages
                .iter()
                .map(|message| message.text.len())
                .sum::<usize>()
                <= MAX_HISTORY_BYTES,
            "bootstrap conversation history is oversized"
        );
        for message in &self.messages {
            message.validate()?;
        }
        Ok(())
    }

    pub(super) fn into_messages(self) -> Vec<ChatMessage> {
        self.messages
            .into_iter()
            .map(StoredMessageV1::into_message)
            .collect()
    }
}

impl StoredMessageV1 {
    pub(super) fn from_message(message: &ChatMessage) -> anyhow::Result<Self> {
        anyhow::ensure!(
            message.text.len() <= MAX_MESSAGE_BYTES,
            "bootstrap conversation message is oversized"
        );
        let text = redact(&message.text);
        anyhow::ensure!(
            text.len() <= MAX_MESSAGE_BYTES,
            "redacted bootstrap conversation message is oversized"
        );
        let stored = Self {
            id: message.id.clone(),
            role: message.role,
            text,
            ts: message.ts,
        };
        stored.validate()?;
        Ok(stored)
    }

    fn into_message(self) -> ChatMessage {
        ChatMessage {
            id: self.id,
            role: self.role,
            text: redact(&self.text),
            ref_item: None,
            lease_renewed_at: None,
            answer_context: None,
            clarification: false,
            ts: self.ts,
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.id.is_empty()
                && self.id.len() <= 128
                && self.id.bytes().all(|byte| byte.is_ascii_graphic()),
            "invalid bootstrap conversation message identity"
        );
        anyhow::ensure!(
            self.text.len() <= MAX_MESSAGE_BYTES,
            "bootstrap conversation message is oversized"
        );
        anyhow::ensure!(
            !self.text.contains('\0'),
            "bootstrap conversation message contains a null character"
        );
        Ok(())
    }
}

pub(super) fn validate_task_id(task_id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        task_id.len() <= 180
            && task_id.starts_with("setup:")
            && task_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_')),
        "invalid machine setup task identity"
    );
    Ok(())
}

pub(super) fn redact(text: &str) -> String {
    let keyed = crate::app::bootstrap_remediation::redact_keyed_values(text);
    crate::error::redact_secrets(&keyed)
}
