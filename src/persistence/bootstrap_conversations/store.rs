use std::{
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::domain::ChatMessage;

use super::{
    identity::{
        ConversationRecordV1, MAX_HISTORY_BYTES, MAX_MESSAGES, MAX_STORE_BYTES, StoredMessageV1,
        validate_task_id,
    },
    storage,
};

/// Access to bounded, redacted machine-local setup conversation histories.
#[derive(Clone, Copy, Debug, Default)]
pub struct BootstrapConversationStore;

impl BootstrapConversationStore {
    /// Load the history bound to one current setup task identity.
    pub fn load(task_id: &str) -> anyhow::Result<Vec<ChatMessage>> {
        let directory = storage::conversation_dir()?;
        load_at(&directory, task_id)
    }

    /// Append messages and return the bounded durable history.
    pub fn append(task_id: &str, messages: &[ChatMessage]) -> anyhow::Result<Vec<ChatMessage>> {
        let directory = storage::conversation_dir()?;
        append_at(&directory, task_id, messages)
    }

    #[cfg(test)]
    pub fn with_test_directory<T>(path: &Path, action: impl FnOnce() -> T) -> T {
        storage::with_test_directory(path, action)
    }
}

fn digest(task_id: &str) -> String {
    format!("{:x}", Sha256::digest(task_id.as_bytes()))
}

fn record_path(directory: &Path, task_id: &str) -> PathBuf {
    directory.join(format!("{}.json", digest(task_id)))
}

fn load_at(directory: &Path, task_id: &str) -> anyhow::Result<Vec<ChatMessage>> {
    validate_task_id(task_id)?;
    storage::ensure_private_directory(directory)?;
    let _lock = storage::lock(directory, &digest(task_id))?;
    let path = record_path(directory, task_id);
    let Some(record) = read_record(&path, task_id)? else {
        return Ok(Vec::new());
    };
    let mut messages = record.into_messages();
    normalize(&mut messages)?;
    let sanitized = ConversationRecordV1::new(task_id, &messages)?;
    write_if_changed(&path, &sanitized)?;
    Ok(messages)
}

fn append_at(
    directory: &Path,
    task_id: &str,
    appended: &[ChatMessage],
) -> anyhow::Result<Vec<ChatMessage>> {
    validate_task_id(task_id)?;
    storage::ensure_private_directory(directory)?;
    let _lock = storage::lock(directory, &digest(task_id))?;
    let path = record_path(directory, task_id);
    let mut messages = read_record(&path, task_id)?
        .map(ConversationRecordV1::into_messages)
        .unwrap_or_default();
    normalize(&mut messages)?;
    for message in appended {
        anyhow::ensure!(
            message.text.len() <= super::identity::MAX_MESSAGE_BYTES,
            "bootstrap conversation message is oversized"
        );
        let sanitized = ChatMessage {
            id: message.id.clone(),
            role: message.role,
            text: super::identity::redact(&message.text),
            ref_item: None,
            answer_context: None,
            clarification: false,
            ts: message.ts,
        };
        StoredMessageV1::from_message(&sanitized)?;
        if !messages.iter().any(|existing| existing.id == sanitized.id) {
            messages.push(sanitized);
        }
    }
    normalize(&mut messages)?;
    let record = ConversationRecordV1::new(task_id, &messages)?;
    write_if_changed(&path, &record)?;
    Ok(messages)
}

fn read_record(path: &Path, task_id: &str) -> anyhow::Result<Option<ConversationRecordV1>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    storage::verify_regular_file(path)?;
    anyhow::ensure!(
        metadata.len() <= MAX_STORE_BYTES as u64,
        "bootstrap conversation store is oversized"
    );
    let bytes = fs::read(path)?;
    anyhow::ensure!(
        bytes.len() <= MAX_STORE_BYTES,
        "bootstrap conversation store is oversized"
    );
    let record: ConversationRecordV1 = serde_json::from_slice(&bytes)?;
    record.validate_for(task_id)?;
    Ok(Some(record))
}

fn write_if_changed(path: &Path, record: &ConversationRecordV1) -> anyhow::Result<()> {
    let json = serde_json::to_string(record)?;
    anyhow::ensure!(
        json.len() <= MAX_STORE_BYTES,
        "bootstrap conversation store is oversized"
    );
    if fs::read_to_string(path).is_ok_and(|current| current == json) {
        return Ok(());
    }
    crate::artifacts::atomic_write(path, &json)?;
    storage::secure_file(path)
}

fn normalize(messages: &mut Vec<ChatMessage>) -> anyhow::Result<()> {
    for message in messages.iter_mut() {
        message.text = super::identity::redact(&message.text);
        message.ref_item = None;
        StoredMessageV1::from_message(message)?;
    }
    messages.sort_by(|left, right| left.ts.cmp(&right.ts).then_with(|| left.id.cmp(&right.id)));
    let mut seen = std::collections::BTreeSet::new();
    messages.retain(|message| seen.insert(message.id.clone()));
    while messages.len() > MAX_MESSAGES
        || messages
            .iter()
            .map(|message| message.text.len())
            .sum::<usize>()
            > MAX_HISTORY_BYTES
    {
        messages.remove(0);
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn append_at_for_test(
    directory: &Path,
    task_id: &str,
    messages: &[ChatMessage],
) -> anyhow::Result<Vec<ChatMessage>> {
    append_at(directory, task_id, messages)
}

#[cfg(test)]
pub(super) fn load_at_for_test(
    directory: &Path,
    task_id: &str,
) -> anyhow::Result<Vec<ChatMessage>> {
    load_at(directory, task_id)
}
