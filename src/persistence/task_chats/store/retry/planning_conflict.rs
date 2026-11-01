use super::reply_claim::stored_history_mut;
use super::*;

impl TaskChats {
    /// End an answer's retry lifecycle after its bounded conflict retries are
    /// exhausted, leaving the saved answer available for internal recovery.
    pub fn mark_planning_conflict_retry_exhausted(
        &mut self,
        slug: &str,
        key: &str,
        user_id: &str,
        retries: usize,
    ) -> Result<bool, String> {
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
        let after = &messages[latest_user + 1..];
        let current_retries = after
            .iter()
            .filter(|message| is_conflict_retry(message))
            .count();
        let claim_exhausted = retries <= MAX_PLANNING_CONFLICT_RETRIES
            && retry_claim_state(messages, latest_user).is_some_and(
                |(current, claim_count, last_claim_fresh)| {
                    current == retries
                        && claim_count >= MAX_PLANNING_CONFLICT_CLAIMS_PER_RETRY
                        && !last_claim_fresh
                },
            );
        if current_retries != retries
            || (retries <= MAX_PLANNING_CONFLICT_RETRIES && !claim_exhausted)
            || after.iter().any(is_substantive_agent_reply)
            || super::has_task_reply_outcome(after)
        {
            return Ok(false);
        }
        let mut outcome = ChatMessage::new(
            ChatRole::System,
            "The saved answer reached its internal planning-conflict recovery limit. Its conversation is retained for recovery; no repeat answer is needed.",
            Some(key.into()),
        );
        TaskChats::mark_task_reply_retry_exhausted(&mut outcome, user_id);
        let outcome_id = outcome.id.clone();
        stored_history_mut(&mut stored, &storage_key)
            .ok_or_else(|| format!("Task conversation {key} disappeared during recovery."))?
            .push(outcome);
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
        Ok(self
            .messages
            .get(key)
            .is_some_and(|history| history.iter().any(|message| message.id == outcome_id)))
    }

    /// Number of durable conflict outcomes after the latest user answer when
    /// that answer is still waiting for an internal retry.
    pub fn pending_planning_conflict_retries(&self, key: &str) -> Option<usize> {
        let messages = self.messages.get(key)?;
        let latest_user = messages
            .iter()
            .rposition(|message| message.role == ChatRole::User && !message.clarification)?;
        let (retries, claim_count, last_claim_fresh) = retry_claim_state(messages, latest_user)?;
        if claim_count >= MAX_PLANNING_CONFLICT_CLAIMS_PER_RETRY || last_claim_fresh {
            return None;
        }
        Some(retries)
    }

    /// A saved retry whose full claim takeover budget expired can no longer
    /// start another worker and must be moved to durable internal recovery.
    pub fn exhausted_planning_conflict_retry_claims(&self, key: &str) -> Option<usize> {
        let messages = self.messages.get(key)?;
        let latest_user = messages
            .iter()
            .rposition(|message| message.role == ChatRole::User && !message.clarification)?;
        let (retries, claim_count, last_claim_fresh) = retry_claim_state(messages, latest_user)?;
        (claim_count >= MAX_PLANNING_CONFLICT_CLAIMS_PER_RETRY && !last_claim_fresh)
            .then_some(retries)
    }

    /// Atomically claim one retry for the latest saved answer across windows.
    /// A single stale-claim takeover recovers from a crashed app without
    /// allowing repeated windows to launch the same planner work forever.
    pub fn claim_pending_planning_conflict_retry(
        &mut self,
        slug: &str,
        key: &str,
        user_id: &str,
        retries: usize,
    ) -> Result<Option<String>, String> {
        if retries == 0 || retries > MAX_PLANNING_CONFLICT_RETRIES {
            return Ok(None);
        }
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
        let messages = stored_history_mut(&mut stored, &storage_key)
            .ok_or_else(|| format!("Task conversation {key} disappeared before retry."))?;
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
        let current_retries = after
            .iter()
            .filter(|message| {
                message.role == ChatRole::System
                    && message.id.starts_with(PLANNING_CONFLICT_RETRY_PREFIX)
            })
            .count();
        let Some(last_conflict) = after.iter().rposition(is_conflict_retry) else {
            return Ok(None);
        };
        if current_retries != retries || after.iter().any(is_substantive_agent_reply) {
            return Ok(None);
        }

        let scope = claim_scope(user_id, retries);
        let trailing = &after[last_conflict + 1..];
        if trailing.iter().any(|message| {
            !(message.role == ChatRole::System && message.id.starts_with(&scope))
                && !(message.role == ChatRole::User && message.clarification)
                && !is_clarification_agent_reply(message)
        }) {
            return Ok(None);
        }
        let claims = trailing
            .iter()
            .filter(|message| message.role == ChatRole::System && message.id.starts_with(&scope))
            .collect::<Vec<_>>();
        if claims.len() >= MAX_PLANNING_CONFLICT_CLAIMS_PER_RETRY
            || claims.last().is_some_and(|claim| claim_is_fresh(&claim.ts))
        {
            return Ok(None);
        }
        let mut claim = ChatMessage::new(
            ChatRole::System,
            "Kool.ad/e is retrying this saved task reply internally.",
            Some(key.into()),
        );
        claim.id = format!("{scope}{}", uuid::Uuid::new_v4());
        let claim_id = claim.id.clone();
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
        Ok(Some(claim_id))
    }

    pub fn mark_planning_conflict_retry(message: &mut ChatMessage) {
        message.id = format!("{PLANNING_CONFLICT_RETRY_PREFIX}{}", uuid::Uuid::new_v4());
    }
}

fn retry_claim_state(messages: &[ChatMessage], latest_user: usize) -> Option<(usize, usize, bool)> {
    let after = &messages[latest_user + 1..];
    if after.iter().any(is_substantive_agent_reply) || super::has_task_reply_outcome(after) {
        return None;
    }
    let retries = after
        .iter()
        .filter(|message| is_conflict_retry(message))
        .count();
    let last_conflict = after.iter().rposition(is_conflict_retry)?;
    let scope = claim_scope(&messages[latest_user].id, retries);
    let trailing = &after[last_conflict + 1..];
    if trailing.iter().any(|message| {
        !(message.role == ChatRole::System && message.id.starts_with(&scope))
            && !(message.role == ChatRole::User && message.clarification)
            && !is_clarification_agent_reply(message)
    }) {
        return None;
    }
    let claims = trailing
        .iter()
        .filter(|message| message.role == ChatRole::System && message.id.starts_with(&scope))
        .collect::<Vec<_>>();
    Some((
        retries,
        claims.len(),
        claims.last().is_some_and(|claim| claim_is_fresh(&claim.ts)),
    ))
}
