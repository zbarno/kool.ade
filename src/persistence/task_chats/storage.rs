use crate::domain::ChatMessage;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) type Histories = BTreeMap<String, Vec<ChatMessage>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredHistories {
    #[serde(default = "current_version")]
    pub(super) schema_version: u32,
    #[serde(default)]
    pub(super) task_identities: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) task_histories: BTreeMap<String, Vec<ChatMessage>>,
    #[serde(default)]
    pub(super) other_histories: Histories,
}

impl Default for StoredHistories {
    fn default() -> Self {
        Self {
            schema_version: current_version(),
            task_identities: BTreeMap::new(),
            task_histories: BTreeMap::new(),
            other_histories: BTreeMap::new(),
        }
    }
}

fn current_version() -> u32 {
    2
}

pub(super) fn read_stored(slug: &str) -> Result<(StoredHistories, bool), String> {
    let path = crate::persistence::project_dir(slug).join("task-conversations.json");
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((StoredHistories::default(), false));
        }
        Err(error) => return Err(error.to_string()),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value.get("schemaVersion").is_some() {
        let stored: StoredHistories = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if stored.schema_version > current_version() {
            return Err(format!(
                "Task conversation store version {} is newer than supported version {}",
                stored.schema_version,
                current_version()
            ));
        }
        return Ok((stored.clone(), stored.schema_version < current_version()));
    }
    let histories: Histories = serde_json::from_value(value).map_err(|e| e.to_string())?;
    Ok((
        StoredHistories {
            other_histories: histories,
            ..Default::default()
        },
        true,
    ))
}

#[cfg(test)]
pub(super) fn read(slug: &str) -> Result<Histories, String> {
    let (stored, _) = read_stored(slug)?;
    let mut histories = stored.other_histories;
    for (uid, messages) in stored.task_histories {
        histories.insert(format!("@unlinked:{uid}"), messages);
    }
    Ok(histories)
}

pub(super) fn merge(histories: &mut Histories, key: &str, messages: &[ChatMessage]) {
    if messages.is_empty() {
        return;
    }
    let history = histories.entry(key.into()).or_default();
    for message in messages {
        if !history.iter().any(|existing| existing.id == message.id) {
            history.push(message.clone());
        }
    }
    history.sort_by(|a, b| a.ts.cmp(&b.ts).then_with(|| a.id.cmp(&b.id)));
}

pub(super) fn merge_identity(
    stored: &mut StoredHistories,
    path: &str,
    uid: &str,
) -> Result<bool, String> {
    if let Some(previous) = stored.task_identities.get(path) {
        if previous != uid {
            // Preserve the historical alias. The current runtime path map is
            // separate, so a reused filename cannot attach the old history
            // to the new task.
            return Ok(false);
        }
        return Ok(false);
    }
    stored.task_identities.insert(path.into(), uid.into());
    Ok(true)
}

pub(super) fn canonicalize(stored: &mut StoredHistories) {
    let aliases = stored.task_identities.clone();
    for (path, uid) in aliases {
        if let Some(history) = stored.other_histories.remove(&path) {
            let target = stored.task_histories.entry(uid).or_default();
            for message in history {
                if !target.iter().any(|old| old.id == message.id) {
                    target.push(message);
                }
            }
            target.sort_by(|a, b| a.ts.cmp(&b.ts).then_with(|| a.id.cmp(&b.id)));
        }
    }
    stored.schema_version = current_version();
}

pub(super) fn display_histories(
    stored: &StoredHistories,
    current_paths: &BTreeMap<String, String>,
) -> Histories {
    let mut histories = stored.other_histories.clone();
    for (uid, messages) in &stored.task_histories {
        let key = current_paths
            .get(uid)
            .cloned()
            .unwrap_or_else(|| format!("@unlinked:{uid}"));
        merge(&mut histories, &key, messages);
    }
    histories
}
