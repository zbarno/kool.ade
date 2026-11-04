use crate::domain::ChatMessage;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) type Histories = BTreeMap<String, Vec<ChatMessage>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StoredHistories {
    pub(super) schema_version: u32,
    pub(super) task_identities: BTreeMap<String, String>,
    pub(super) task_histories: BTreeMap<String, Vec<ChatMessage>>,
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

pub(super) fn read_stored(slug: &str) -> Result<StoredHistories, String> {
    let path = crate::persistence::project_dir(slug).join("task-conversations.json");
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StoredHistories::default());
        }
        Err(error) => return Err(error.to_string()),
    };
    let stored: StoredHistories = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if stored.schema_version != current_version() {
        return Err(format!(
            "Unsupported task conversation store version {}",
            stored.schema_version
        ));
    }
    Ok(stored)
}

#[cfg(test)]
pub(super) fn read(slug: &str) -> Result<Histories, String> {
    read_stored(slug).map(|stored| {
        display_histories(
            &stored,
            &BTreeMap::new(),
            &std::collections::BTreeSet::new(),
        )
    })
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

pub(super) fn merge_identity(stored: &mut StoredHistories, path: &str, uid: &str) {
    if stored
        .task_identities
        .get(path)
        .is_none_or(|previous| previous != uid)
    {
        // The old UID stream remains unlinked; do not bind it to a reused
        // display path.
        stored.task_identities.insert(path.into(), uid.into());
    }
}

pub(super) fn display_histories(
    stored: &StoredHistories,
    current_paths: &BTreeMap<String, String>,
    excluded_paths: &std::collections::BTreeSet<String>,
) -> Histories {
    let hidden = stored
        .task_identities
        .keys()
        .chain(excluded_paths.iter())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let mut histories = stored
        .other_histories
        .iter()
        .filter(|(key, _)| !hidden.contains(*key))
        .map(|(key, messages)| (key.clone(), messages.clone()))
        .collect::<Histories>();
    for (uid, messages) in &stored.task_histories {
        let key = current_paths
            .get(uid)
            .cloned()
            .unwrap_or_else(|| format!("@unlinked:{uid}"));
        merge(&mut histories, &key, messages);
    }
    histories
}
