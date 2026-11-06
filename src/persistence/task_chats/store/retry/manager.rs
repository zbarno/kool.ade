//! One fenced Manager recovery after ordinary saved-answer recovery stops.
use super::reply_claim::stored_history_mut;
use super::*;
pub(super) const PREFIX: &str = "internal-manager-recovery:";

pub(super) fn after_authorization(messages: &[ChatMessage]) -> &[ChatMessage] {
    messages
        .iter()
        .rposition(|m| m.role == ChatRole::System && m.id.starts_with(PREFIX))
        .map(|index| &messages[index..])
        .unwrap_or(messages)
}

fn source_key_matches(stored: &super::super::StoredHistories, key: &str, uid: &str) -> bool {
    match stored.task_identities.get(key) {
        Some(current) => current == uid,
        None => key == format!("task:{uid}") && uuid::Uuid::parse_str(uid).is_ok(),
    }
}

impl TaskChats {
    /// Hold the conversation lock while native repair inspects an exact failed reply.
    pub(crate) fn lock_failed_reply_for_recovery(
        slug: &str,
        key: &str,
        source_uid: &str,
        reply_id: &str,
    ) -> Result<Option<TaskReplyLock>, String> {
        let lock = Self::lock_store(slug)?;
        let stored = super::super::read_stored(slug)?;
        if !source_key_matches(&stored, key, source_uid) {
            return Ok(None);
        }
        let Some(messages) = stored_history(&stored, key) else {
            return Ok(None);
        };
        let Some(index) = messages
            .iter()
            .rposition(|m| m.role == ChatRole::User && !m.clarification)
            .filter(|index| messages[*index].id == reply_id)
        else {
            return Ok(None);
        };
        if !has_failed_task_reply(messages)
            || has_active_reply_claim(messages)
            || messages[index + 1..].iter().any(is_substantive_agent_reply)
        {
            return Ok(None);
        }
        Ok(Some(TaskReplyLock {
            _lock: lock,
            slug: slug.into(),
            key: key.into(),
            user_id: reply_id.into(),
            reply_claim_id: None,
        }))
    }

    /// Authorize one replay only for the exact still-unapplied answer and identity.
    /// The normal leased claim and final-write guards remain mandatory.
    pub(crate) fn authorize_manager_recovery(
        &mut self,
        slug: &str,
        key: &str,
        source_uid: &str,
        reply_id: &str,
    ) -> Result<bool, String> {
        let _lock = Self::lock_store(slug)?;
        let mut stored = super::super::read_stored(slug)?;
        if self.has_ambiguous_identity(key) || !source_key_matches(&stored, key, source_uid) {
            return Ok(false);
        }
        let storage_key = if stored.task_identities.contains_key(key) {
            format!("@task:{source_uid}")
        } else {
            key.to_owned()
        };
        let messages = stored_history_mut(&mut stored, &storage_key)
            .ok_or("Recovery conversation is missing")?;
        let Some(index) = messages
            .iter()
            .rposition(|m| m.role == ChatRole::User && !m.clarification)
            .filter(|index| messages[*index].id == reply_id)
        else {
            return Ok(false);
        };
        let after = &messages[index + 1..];
        if !has_failed_task_reply(messages)
            || has_active_reply_claim(messages)
            || after.iter().any(|m| m.id.starts_with(PREFIX))
            || after.iter().any(is_substantive_agent_reply)
        {
            return Ok(false);
        }
        let mut marker = ChatMessage::new(
            ChatRole::System,
            "Manager triage has checked this saved answer for one bounded recovery. Preserve its submitted choice and immutable feature identity. Validate before applying; do not ask the user to repeat the answer.",
            Some(key.into()),
        );
        marker.id = format!("{PREFIX}{reply_id}");
        messages.push(marker);
        let json = serde_json::to_string(&stored).map_err(|e| e.to_string())?;
        crate::artifacts::atomic_write(
            &crate::persistence::project_dir(slug).join("task-conversations.json"),
            &json,
        )
        .map_err(|e| e.to_string())?;
        self.stored = stored;
        self.project_messages();
        self.refreshed = Some(std::time::Instant::now());
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Production boundary: Manager replay crosses the shared durable chat lock,
    /// historical conflict epochs and leased final-write guard. Local temporary
    /// files only; low setup cost. A unit validator cannot prove cross-window fencing.
    #[test]
    fn manager_recovery_fences_old_conflicts_and_allows_one_exact_reply() {
        let dir = tempfile::tempdir().unwrap();
        let slug = dir.path().to_str().unwrap();
        let key = "task:recovery";
        let uid = uuid::Uuid::new_v4().to_string();
        let mut chats = TaskChats::default();
        chats
            .bind_conversation_identities(&[(key.into(), uid.clone())])
            .unwrap();
        chats.ensure_loaded(slug);
        let user = ChatMessage::new(ChatRole::User, "Keep the submitted choice", None);
        let reply = user.id.clone();
        chats.append_user_reply_and_claim(slug, key, user).unwrap();
        let mut conflict = ChatMessage::new(ChatRole::System, "Concurrent planning write", None);
        TaskChats::mark_planning_conflict_retry(&mut conflict);
        let mut failed = ChatMessage::new(ChatRole::System, "Internal recovery stopped", None);
        TaskChats::mark_task_reply_outcome(&mut failed);
        chats.append(slug, key, vec![conflict, failed]).unwrap();
        assert!(
            !chats
                .authorize_manager_recovery(slug, key, &uuid::Uuid::new_v4().to_string(), &reply)
                .unwrap()
        );
        assert!(
            chats
                .authorize_manager_recovery(slug, key, &uid, &reply)
                .unwrap()
        );
        assert!(chats.pending_planning_conflict_retries(key).is_none());
        let claim = chats
            .claim_pending_task_reply(slug, key, &reply)
            .unwrap()
            .unwrap();
        let mut peer = TaskChats::default();
        peer.bind_conversation_identities(&[(key.into(), uid.clone())])
            .unwrap();
        peer.ensure_loaded(slug);
        assert!(
            !peer
                .authorize_manager_recovery(slug, key, &uid, &reply)
                .unwrap()
        );
        assert!(
            peer.claim_pending_task_reply(slug, key, &reply)
                .unwrap()
                .is_none()
        );
        let lock = TaskChats::lock_latest_user_message(slug, key, &reply, Some(&claim))
            .unwrap()
            .unwrap();
        lock.mark_applied().unwrap();
        drop(lock);
        peer.refresh_now(slug);
        assert!(!peer.has_failed_task_reply(key));
        assert!(
            TaskChats::lock_failed_reply_for_recovery(slug, key, &uid, &reply)
                .unwrap()
                .is_none()
        );
        assert!(
            !peer
                .authorize_manager_recovery(slug, key, &uid, &reply)
                .unwrap()
        );
    }
}
