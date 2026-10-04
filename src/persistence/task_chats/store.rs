//! Identity-keyed durable conversations, projected onto current board paths.
use super::storage::{
    StoredHistories, canonicalize, display_histories, merge, merge_identity, read_stored,
};
use crate::{artifacts::task_docs::TaskDocument, domain::ChatMessage};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct TaskChats {
    pub messages: BTreeMap<String, Vec<ChatMessage>>,
    pub drafts: BTreeMap<String, String>,
    pub active: Option<String>,
    pub error: Option<String>,
    pending: Vec<(String, Vec<ChatMessage>)>,
    last_slug: Option<String>,
    loaded: bool,
    refreshed: Option<std::time::Instant>,
    updates: Vec<String>,
    identities: BTreeMap<String, String>,
    paths: BTreeMap<String, String>,
    stored: StoredHistories,
}

impl TaskChats {
    /// Refresh the runtime path projection. Histories themselves are keyed by
    /// task UID on disk, so a moved story immediately keeps its conversation.
    pub fn bind_task_documents(&mut self, docs: &[TaskDocument]) -> Result<(), String> {
        let mut identities = BTreeMap::new();
        let mut paths = BTreeMap::new();
        for doc in docs {
            let Some(identity) = &doc.identity else {
                continue;
            };
            if let Some(previous) = paths.insert(identity.uid.clone(), doc.path.clone())
                && previous != doc.path
            {
                return Err(format!(
                    "Task identity {} appears at both {previous} and {}",
                    identity.uid, doc.path
                ));
            }
            identities.insert(doc.path.clone(), identity.uid.clone());
        }
        self.identities = identities;
        self.paths = paths;
        if self.loaded {
            self.add_current_aliases()?;
            canonicalize(&mut self.stored);
            self.messages = display_histories(&self.stored, &self.paths);
        }
        Ok(())
    }

    fn add_current_aliases(&mut self) -> Result<bool, String> {
        let mut changed = false;
        for (path, uid) in &self.identities {
            changed |= merge_identity(&mut self.stored, path, uid)?;
        }
        Ok(changed)
    }

    fn storage_key(&self, key: &str, stored: &StoredHistories) -> String {
        self.identities
            .get(key)
            .or_else(|| stored.task_identities.get(key))
            .map(|uid| format!("@task:{uid}"))
            .or_else(|| {
                key.strip_prefix("@unlinked:")
                    .map(|uid| format!("@task:{uid}"))
            })
            .unwrap_or_else(|| key.to_owned())
    }

    fn merge_key(stored: &mut StoredHistories, key: &str, messages: &[ChatMessage]) {
        if let Some(uid) = key.strip_prefix("@task:") {
            merge(&mut stored.task_histories, uid, messages);
        } else {
            merge(&mut stored.other_histories, key, messages);
        }
    }

    fn project_messages(&mut self) {
        self.messages = display_histories(&self.stored, &self.paths);
    }

    pub fn take_updates(&mut self) -> Vec<String> {
        std::mem::take(&mut self.updates)
    }

    /// A project-wide view for the main planner, built from the durable task
    /// streams rather than copying them into Main Chat. Explicitly mentioned
    /// tasks get first claim on the budget, then the most recent interactions.
    pub fn project_context(&self, focus: &str, budget: usize) -> String {
        super::context::build(&self.messages, focus, budget)
    }

    pub fn ensure_loaded(&mut self, slug: &str) {
        if self.loaded && self.refreshed.is_some_and(|t| t.elapsed().as_secs() < 1) {
            return;
        }
        self.refreshed = Some(std::time::Instant::now());
        match read_stored(slug) {
            Ok((mut stored, migrated)) => {
                let old_messages = display_histories(&stored, &self.paths);
                for (path, uid) in &self.identities {
                    if let Err(error) = merge_identity(&mut stored, path, uid) {
                        self.error = Some(error);
                        return;
                    }
                }
                canonicalize(&mut stored);
                let aliases_changed =
                    stored.task_identities.len() > self.stored.task_identities.len();
                let previous = self
                    .messages
                    .values()
                    .flat_map(|messages| messages.iter())
                    .map(|m| m.id.as_str())
                    .collect::<std::collections::HashSet<_>>();
                let current = display_histories(&stored, &self.paths);
                if self.loaded {
                    for (key, history) in &current {
                        let added = history
                            .iter()
                            .filter(|message| !previous.contains(message.id.as_str()))
                            .map(|message| {
                                format!(
                                    "{:?}: {}",
                                    message.role,
                                    crate::core::context_build::clip(&message.text, 1600)
                                )
                            })
                            .collect::<Vec<_>>();
                        if !added.is_empty() {
                            self.updates.push(format!(
                                "Task conversation {key} updated in another window: {}",
                                added.join("\n")
                            ));
                        }
                    }
                }
                for (key, pending) in &self.pending {
                    let storage_key = self.storage_key(key, &stored);
                    Self::merge_key(&mut stored, &storage_key, pending);
                }
                canonicalize(&mut stored);
                self.stored = stored;
                self.project_messages();
                self.loaded = true;
                if self.pending.is_empty() {
                    self.error = None;
                }
                let has_histories = !self.stored.task_histories.is_empty()
                    || !self.stored.other_histories.is_empty();
                if (migrated || old_messages != self.messages || (aliases_changed && has_histories))
                    && let Err(error) = self.persist(slug, "", &[])
                {
                    self.error = Some(format!(
                        "Task conversation migration could not be saved: {error}"
                    ));
                }
            }
            Err(error) => self.error = Some(format!("Cannot read task conversations: {error}")),
        }
    }

    pub fn refresh_now(&mut self, slug: &str) {
        self.refreshed = None;
        self.ensure_loaded(slug);
    }

    fn lock_store(slug: &str) -> Result<std::fs::File, String> {
        let dir = crate::persistence::project_dir(slug);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("task-conversations.lock"))
            .map_err(|e| e.to_string())?;
        if lock.try_lock().is_err() {
            for delay_ms in [4u64, 8, 12, 15, 15, 15] {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                if lock.try_lock().is_ok() {
                    break;
                }
            }
        }
        lock.try_lock().map_err(|_| {
            "Another window is saving a task conversation; retry shortly.".to_string()
        })?;
        Ok(lock)
    }

    fn persist(&mut self, slug: &str, key: &str, messages: &[ChatMessage]) -> Result<(), String> {
        self.last_slug = Some(slug.to_owned());
        let dir = crate::persistence::project_dir(slug);
        let _lock = Self::lock_store(slug)?;
        let (mut next, _) = read_stored(slug)?;
        for (path, uid) in &self.identities {
            merge_identity(&mut next, path, uid)?;
        }
        canonicalize(&mut next);
        for (pending_key, pending) in &self.pending {
            let storage_key = self.storage_key(pending_key, &next);
            Self::merge_key(&mut next, &storage_key, pending);
        }
        let storage_key = self.storage_key(key, &next);
        Self::merge_key(&mut next, &storage_key, messages);
        canonicalize(&mut next);
        let json = serde_json::to_string(&next).map_err(|e| e.to_string())?;
        crate::artifacts::atomic_write(&dir.join("task-conversations.json"), &json)
            .map_err(|e| e.to_string())?;
        self.stored = next;
        self.project_messages();
        self.loaded = true;
        self.pending.clear();
        self.refreshed = Some(std::time::Instant::now());
        self.error = None;
        Ok(())
    }

    pub fn append(
        &mut self,
        slug: &str,
        key: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<(), String> {
        self.persist(slug, key, &messages).map_err(|error| {
            self.error = Some(format!("Task conversation could not be saved: {error}"));
            error
        })
    }

    /// An already-completed agent reply must remain visible if storage fails.
    pub fn remember_response(&mut self, slug: &str, key: &str, messages: Vec<ChatMessage>) {
        if self.append(slug, key, messages.clone()).is_err() {
            merge(&mut self.messages, key, &messages);
            self.pending.push((key.into(), messages));
        }
    }

    pub fn retry_save(&mut self, slug: &str) {
        self.ensure_loaded(slug);
        self.drain_if_pending(slug);
    }

    /// Best-effort background delivery of replies that a failed save queued.
    pub fn drain_if_pending(&mut self, slug: &str) {
        if self.pending.is_empty() {
            return;
        }
        if self.persist(slug, "", &[]).is_ok() {
            self.pending.clear();
        }
    }
}

impl Drop for TaskChats {
    fn drop(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        if let Some(slug) = self.last_slug.clone() {
            let _ = self.persist(&slug, "", &[]);
        }
    }
}
