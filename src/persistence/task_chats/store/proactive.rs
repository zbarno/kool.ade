use super::{TaskChats, canonicalize, detach_other_history, merge_identity, read_stored};
use crate::domain::{ChatMessage, ChatRole};

#[cfg(test)]
#[path = "proactive/tests.rs"]
mod tests;

impl TaskChats {
    /// Save a proactive prompt only if no answer arrived in another window
    /// after the caller's preliminary attention check.
    pub fn remember_proactive_response_if_unanswered(
        &mut self,
        slug: &str,
        key: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<bool, String> {
        if messages.is_empty()
            || messages
                .iter()
                .any(|message| message.role != ChatRole::Agent || message.clarification)
        {
            return Err("Proactive task prompts must contain agent messages only.".into());
        }
        if self.ambiguous_paths.contains(key) {
            return Ok(false);
        }

        self.last_slug = Some(slug.to_owned());
        let dir = crate::persistence::project_dir(slug);
        let _lock = Self::lock_store(slug)?;
        let (mut next, _) = read_stored(slug)?;
        for path in &self.ambiguous_paths {
            detach_other_history(&mut next, path, "legacy");
        }
        for (path, uid) in &self.identities {
            merge_identity(
                &mut next,
                path,
                uid,
                self.legacy_migration_keys.contains(path),
            )?;
        }
        canonicalize(&mut next);
        for (pending_key, pending) in &self.pending {
            let storage_key = self.storage_key(pending_key, &next);
            Self::merge_key(&mut next, &storage_key, pending);
        }

        let storage_key = self.storage_key(key, &next);
        let current = super::retry::stored_history(&next, &storage_key)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let latest_user = current
            .iter()
            .rposition(|message| message.role == ChatRole::User && !message.clarification);
        let latest_agent = current
            .iter()
            .rposition(|message| message.role == ChatRole::Agent && !message.clarification);
        let unanswered_user = latest_user.is_some_and(|user| {
            latest_agent.is_none_or(|agent| user > agent)
                && !super::retry::has_task_reply_outcome(&current[user + 1..])
        });
        let failed_reply = super::retry::has_failed_task_reply(current);
        let duplicate_prompt = messages
            .iter()
            .any(|message| current.iter().any(|saved| saved.id == message.id));
        if unanswered_user || failed_reply || duplicate_prompt {
            self.stored = next;
            self.project_messages();
            self.loaded = true;
            self.refreshed = Some(std::time::Instant::now());
            return Ok(false);
        }

        Self::merge_key(&mut next, &storage_key, &messages);
        canonicalize(&mut next);
        let json = serde_json::to_string(&next).map_err(|error| error.to_string())?;
        crate::artifacts::atomic_write(&dir.join("task-conversations.json"), &json)
            .map_err(|error| error.to_string())?;
        self.stored = next;
        self.project_messages();
        self.loaded = true;
        self.pending.clear();
        self.refreshed = Some(std::time::Instant::now());
        self.error = None;
        Ok(true)
    }
}
