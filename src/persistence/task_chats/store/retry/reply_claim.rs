use super::*;

/// Keeps the conversation stable until a task reply has crossed its planning
/// store write boundary.
pub struct TaskReplyLock {
    #[allow(dead_code)]
    pub(super) _lock: std::fs::File,
    pub(super) slug: String,
    pub(super) key: String,
    pub(super) user_id: String,
    pub(super) reply_claim_id: Option<String>,
}

impl TaskReplyLock {
    /// Mark retry completion before releasing the cross-window conversation lock.
    pub fn mark_applied(&self) -> Result<(), String> {
        let Some(claim_id) = self.reply_claim_id.as_deref() else {
            return Ok(());
        };
        let (mut stored, _) = super::super::read_stored(&self.slug)?;
        super::super::canonicalize(&mut stored);
        let storage_key = stored
            .task_identities
            .get(&self.key)
            .map(|uid| format!("@task:{uid}"))
            .unwrap_or_else(|| self.key.clone());
        let messages = stored_history_mut(&mut stored, &storage_key).ok_or_else(|| {
            format!(
                "Task conversation {} disappeared after its reply was applied.",
                self.key
            )
        })?;
        if current_reply_claim_id(messages, &self.user_id) != Some(claim_id) {
            return Err(
                "The saved task reply retry claim changed before completion could be recorded."
                    .into(),
            );
        }
        let marker_id = format!("{TASK_REPLY_OUTCOME_PREFIX}applied:{claim_id}");
        if messages.iter().any(|message| message.id == marker_id) {
            return Ok(());
        }
        let mut marker = ChatMessage::new(
            ChatRole::System,
            "The saved task reply was applied; syncing its result to the conversation.",
            Some(self.key.clone()),
        );
        marker.id = marker_id;
        messages.push(marker);
        super::super::canonicalize(&mut stored);
        let json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
        let path = crate::persistence::project_dir(&self.slug).join("task-conversations.json");
        crate::artifacts::atomic_write(&path, &json).map_err(|error| error.to_string())
    }
}

impl TaskChats {
    /// Persist a substantive user reply and its recovery lease in one atomic
    /// write, so a crash cannot leave an unclaimed turn between the two.
    pub fn append_user_reply_and_claim(
        &mut self,
        slug: &str,
        key: &str,
        user: ChatMessage,
    ) -> Result<String, String> {
        if user.role != ChatRole::User || user.clarification {
            return Err("Only a submitted user answer can claim a task reply turn.".into());
        }
        let user_id = user.id.clone();
        let _lock = Self::lock_store(slug)?;
        let (mut stored, _) = super::super::read_stored(slug)?;
        for path in &self.ambiguous_paths {
            super::super::detach_other_history(&mut stored, path, "legacy");
        }
        for (path, uid) in &self.identities {
            super::super::merge_identity(
                &mut stored,
                path,
                uid,
                self.legacy_migration_keys.contains(path),
            )?;
        }
        super::super::canonicalize(&mut stored);
        let storage_key = self.storage_key(key, &stored);
        if stored_history(&stored, &storage_key)
            .is_some_and(|history| has_active_reply_claim(history))
        {
            return Err(
                "A saved task reply is already being processed in another window; your draft is preserved.".into(),
            );
        }
        for (pending_key, pending) in &self.pending {
            let pending_storage_key = self.storage_key(pending_key, &stored);
            Self::merge_key(&mut stored, &pending_storage_key, pending);
        }
        Self::merge_key(&mut stored, &storage_key, &[user]);
        let messages = stored_history_mut(&mut stored, &storage_key)
            .ok_or_else(|| format!("Task conversation {key} could not be saved."))?;
        let latest_user = messages
            .iter()
            .rposition(|message| message.role == ChatRole::User && !message.clarification)
            .filter(|index| messages[*index].id == user_id)
            .ok_or_else(|| format!("Task reply for {key} was superseded before it could start."))?;
        let mut claim = ChatMessage::new(
            ChatRole::System,
            "Kool.ad/e is processing this saved task reply internally.",
            Some(key.into()),
        );
        claim.id = format!(
            "{TASK_REPLY_CLAIM_PREFIX}{user_id}:{}",
            uuid::Uuid::new_v4()
        );
        let claim_id = claim.id.clone();
        messages.insert(latest_user + 1, claim);
        super::super::canonicalize(&mut stored);
        let dir = crate::persistence::project_dir(slug);
        let json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
        crate::artifacts::atomic_write(&dir.join("task-conversations.json"), &json)
            .map_err(|error| error.to_string())?;
        self.last_slug = Some(slug.to_owned());
        self.stored = stored;
        self.project_messages();
        self.loaded = true;
        self.pending.clear();
        self.refreshed = Some(std::time::Instant::now());
        self.error = None;
        Ok(claim_id)
    }

    /// Claim a saved answer whose worker was lost or whose app exited before
    /// it could finish. A shared lease prevents another window from starting
    /// the same task while its owner is still live.
    pub fn claim_pending_task_reply(
        &mut self,
        slug: &str,
        key: &str,
        user_id: &str,
    ) -> Result<Option<String>, String> {
        let _lock = Self::lock_store(slug)?;
        let (mut stored, _) = super::super::read_stored(slug)?;
        for path in &self.ambiguous_paths {
            super::super::detach_other_history(&mut stored, path, "legacy");
        }
        for (path, uid) in &self.identities {
            super::super::merge_identity(
                &mut stored,
                path,
                uid,
                self.legacy_migration_keys.contains(path),
            )?;
        }
        for (pending_key, pending) in &self.pending {
            let storage_key = self.storage_key(pending_key, &stored);
            Self::merge_key(&mut stored, &storage_key, pending);
        }
        super::super::canonicalize(&mut stored);
        let storage_key = self.storage_key(key, &stored);
        let Some(messages) = stored_history_mut(&mut stored, &storage_key) else {
            return Ok(None);
        };
        let Some(latest_user) = messages
            .iter()
            .rposition(|message| message.role == ChatRole::User && !message.clarification)
        else {
            return Ok(None);
        };
        if messages[latest_user].id != user_id {
            return Ok(None);
        }
        let after = &messages[latest_user + 1..];
        if after.iter().any(is_substantive_agent_reply)
            || after.iter().any(is_conflict_retry)
            || has_task_reply_outcome(after)
        {
            return Ok(None);
        }
        if let Some(claim_id) = task_reply_claim_id(messages, user_id)
            && let Some(claim) = messages.iter().find(|message| message.id == claim_id)
            && claim_is_fresh(claim.lease_renewed_at.as_ref().unwrap_or(&claim.ts))
        {
            return Ok(None);
        }
        let exhausted = after
            .iter()
            .position(super::safe_recovery::is_recoverable)
            .is_some_and(|failure| {
                after[failure + 1..].iter().any(|message| {
                    message.role == ChatRole::System
                        && message.id.starts_with(TASK_REPLY_CLAIM_PREFIX)
                })
            });
        let mut claim = ChatMessage::new(
            ChatRole::System,
            if exhausted {
                "The internal recovery limit was reached. Your original answer remains saved; internal investigation is required. Do not submit the answer again."
            } else {
                "Kool.ad/e is processing this saved task reply internally."
            },
            Some(key.into()),
        );
        let claim_id = if exhausted {
            Self::mark_task_reply_retry_exhausted(&mut claim, user_id);
            None
        } else {
            claim.id = format!(
                "{TASK_REPLY_CLAIM_PREFIX}{user_id}:{}",
                uuid::Uuid::new_v4()
            );
            Some(claim.id.clone())
        };
        messages.push(claim);
        super::super::canonicalize(&mut stored);
        let json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
        let dir = crate::persistence::project_dir(slug);
        crate::artifacts::atomic_write(&dir.join("task-conversations.json"), &json)
            .map_err(|error| error.to_string())?;
        self.last_slug = Some(slug.to_owned());
        self.stored = stored;
        self.project_messages();
        self.loaded = true;
        self.pending.clear();
        self.refreshed = Some(std::time::Instant::now());
        self.error = None;
        Ok(claim_id)
    }

    /// Renew a live retry claim. Tick polling keeps long-running model calls
    /// from appearing abandoned to another application window.
    pub fn renew_task_reply_claim(
        &mut self,
        slug: &str,
        key: &str,
        user_id: &str,
        claim_id: &str,
    ) -> Result<bool, String> {
        let _lock = Self::lock_store(slug)?;
        let (mut stored, _) = super::super::read_stored(slug)?;
        super::super::canonicalize(&mut stored);
        let storage_key = self.storage_key(key, &stored);
        let Some(messages) = stored_history_mut(&mut stored, &storage_key) else {
            return Ok(false);
        };
        let Some(latest_user) = messages
            .iter()
            .rposition(|message| message.role == ChatRole::User && !message.clarification)
        else {
            return Ok(false);
        };
        if messages[latest_user].id != user_id {
            return Ok(false);
        }
        if current_reply_claim_id(messages, user_id) != Some(claim_id) {
            return Ok(false);
        }
        let Some(claim) = messages[latest_user + 1..]
            .iter_mut()
            .find(|message| message.role == ChatRole::System && message.id == claim_id)
        else {
            return Ok(false);
        };
        claim.lease_renewed_at = Some(chrono::Utc::now());
        let json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
        let dir = crate::persistence::project_dir(slug);
        crate::artifacts::atomic_write(&dir.join("task-conversations.json"), &json)
            .map_err(|error| error.to_string())?;
        self.last_slug = Some(slug.to_owned());
        self.stored = stored;
        self.project_messages();
        self.loaded = true;
        self.refreshed = Some(std::time::Instant::now());
        Ok(true)
    }

    /// Mark a terminal worker outcome so a restart does not replay it as an
    /// orphaned user reply.
    pub fn mark_task_reply_outcome(message: &mut ChatMessage) {
        message.id = format!("{TASK_REPLY_FAILED_PREFIX}{}", uuid::Uuid::new_v4());
    }

    pub fn mark_task_reply_retry_exhausted(message: &mut ChatMessage, user_id: &str) {
        message.id = format!("{TASK_REPLY_EXHAUSTED_PREFIX}{user_id}");
    }
}

pub(super) fn stored_history_mut<'a>(
    stored: &'a mut super::super::StoredHistories,
    key: &str,
) -> Option<&'a mut Vec<ChatMessage>> {
    if let Some(uid) = stored.task_identities.get(key) {
        stored.task_histories.get_mut(uid)
    } else if let Some(uid) = key.strip_prefix("@task:") {
        stored.task_histories.get_mut(uid)
    } else {
        stored.other_histories.get_mut(key)
    }
}

pub(super) fn current_reply_claim_id<'a>(
    messages: &'a [ChatMessage],
    user_id: &str,
) -> Option<&'a str> {
    current_retry_claim_id(messages, user_id).or_else(|| task_reply_claim_id(messages, user_id))
}
