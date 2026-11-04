//! Identity-keyed durable conversations, projected onto current board paths.
use super::storage::{StoredHistories, display_histories, merge, merge_identity, read_stored};
use crate::domain::ChatMessage;
use std::collections::BTreeMap;

mod identity;
mod lock;
mod proactive;
mod retry;

pub use retry::{MAX_PLANNING_CONFLICT_RETRIES, PLANNING_CONFLICT_RETRY_PREFIX, TaskReplyLock};

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
    ambiguous_paths: std::collections::BTreeSet<String>,
    stored: StoredHistories,
}

impl TaskChats {
    fn storage_key(&self, key: &str, stored: &StoredHistories) -> String {
        self.identities
            .get(key)
            .filter(|_| !self.ambiguous_paths.contains(key))
            .or_else(|| stored.task_identities.get(key))
            .filter(|_| !self.ambiguous_paths.contains(key))
            .map(|uid| format!("@task:{uid}"))
            .or_else(|| {
                self.ambiguous_paths
                    .contains(key)
                    .then(|| format!("@ambiguous:{key}"))
            })
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
        self.messages =
            display_histories(&self.stored, &self.paths, &self.excluded_history_paths());
    }

    fn excluded_history_paths(&self) -> std::collections::BTreeSet<String> {
        self.identities
            .keys()
            .chain(self.ambiguous_paths.iter())
            .cloned()
            .collect()
    }

    pub fn take_updates(&mut self) -> Vec<String> {
        std::mem::take(&mut self.updates)
    }

    /// A project-wide view for the Manager, built from durable task streams
    /// rather than copying them into Main Chat. Mentioned tasks get first claim
    /// on the budget, then the most recent interactions.
    pub fn project_context(&self, focus: &str, budget: usize) -> String {
        super::context::build(&self.messages, focus, budget)
    }

    /// Context for one planning request is limited to conversations it names.
    /// The Manager uses `project_context` to coordinate across the whole board.
    pub fn project_context_for_focus(&self, focus: &str, budget: usize) -> String {
        super::context::build_focused(&self.messages, focus, budget)
    }

    pub fn ensure_loaded(&mut self, slug: &str) {
        if self.loaded && self.refreshed.is_some_and(|t| t.elapsed().as_secs() < 1) {
            return;
        }
        self.refreshed = Some(std::time::Instant::now());
        match read_stored(slug) {
            Ok(mut stored) => {
                let excluded = self.excluded_history_paths();
                let old_messages = display_histories(&stored, &self.paths, &excluded);
                for (path, uid) in &self.identities {
                    merge_identity(&mut stored, path, uid);
                }
                let aliases_changed =
                    stored.task_identities.len() > self.stored.task_identities.len();
                let previous = self
                    .messages
                    .values()
                    .flat_map(|messages| messages.iter())
                    .map(|m| m.id.as_str())
                    .collect::<std::collections::HashSet<_>>();
                let current = display_histories(&stored, &self.paths, &excluded);
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
                self.stored = stored;
                self.project_messages();
                self.loaded = true;
                if self.pending.is_empty() {
                    self.error = None;
                }
                let has_histories = !self.stored.task_histories.is_empty()
                    || !self.stored.other_histories.is_empty();
                if (old_messages != self.messages || (aliases_changed && has_histories))
                    && let Err(error) = self.persist(slug, "", &[])
                {
                    self.error = Some(format!(
                        "Task conversation state could not be saved: {error}"
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

    fn persist(&mut self, slug: &str, key: &str, messages: &[ChatMessage]) -> Result<(), String> {
        self.last_slug = Some(slug.to_owned());
        let dir = crate::persistence::project_dir(slug);
        let _lock = Self::lock_store(slug)?;
        let mut next = read_stored(slug)?;
        for (path, uid) in &self.identities {
            merge_identity(&mut next, path, uid);
        }
        let storage_key = self.storage_key(key, &next);
        let new_user_message = messages.iter().any(|message| {
            message.role == crate::domain::ChatRole::User
                && !message.clarification
                && retry::stored_history(&next, &storage_key)
                    .is_none_or(|history| history.iter().all(|previous| previous.id != message.id))
        });
        if new_user_message
            && retry::stored_history(&next, &storage_key)
                .is_some_and(|history| retry::has_active_reply_claim(history))
        {
            return Err(
                "This saved task reply is being retried internally. Your new reply remains a draft until that retry finishes.".into(),
            );
        }
        let pending_user_blocked = self.pending.iter().any(|(pending_key, pending)| {
            let pending_storage_key = self.storage_key(pending_key, &next);
            pending.iter().any(|message| {
                message.role == crate::domain::ChatRole::User && !message.clarification
            }) && retry::stored_history(&next, &pending_storage_key)
                .is_some_and(|history| retry::has_active_reply_claim(history))
        });
        if pending_user_blocked {
            return Err(
                "A saved user reply is waiting for an internal retry to finish before it can be persisted.".into(),
            );
        }
        for (pending_key, pending) in &self.pending {
            let pending_storage_key = self.storage_key(pending_key, &next);
            Self::merge_key(&mut next, &pending_storage_key, pending);
        }
        Self::merge_key(&mut next, &storage_key, messages);
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
