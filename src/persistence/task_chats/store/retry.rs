use super::TaskChats;
use crate::domain::{ChatMessage, ChatRole};

pub const PLANNING_CONFLICT_RETRY_PREFIX: &str = "internal-retry:planning-write-conflict:";
const PLANNING_CONFLICT_CLAIM_PREFIX: &str = "internal-retry:planning-write-claim:";
const PLANNING_CONFLICT_APPLIED_PREFIX: &str = "internal-retry:planning-write-applied:";
const TASK_REPLY_CLAIM_PREFIX: &str = "internal-task-reply-claim:";
const TASK_REPLY_OUTCOME_PREFIX: &str = "internal-task-reply-outcome:";
const TASK_REPLY_FAILED_PREFIX: &str = "internal-task-reply-outcome:failed:";
const TASK_REPLY_EXHAUSTED_PREFIX: &str = "internal-task-reply-outcome:exhausted:";
const TASK_REPLY_CLAIM_LEASE: std::time::Duration = std::time::Duration::from_secs(15 * 60);
pub const MAX_PLANNING_CONFLICT_RETRIES: usize = 1;
const MAX_PLANNING_CONFLICT_CLAIMS_PER_RETRY: usize = 2;

mod helpers;
mod planning_conflict;
mod reply_claim;

use helpers::{
    claim_is_fresh, claim_scope, is_clarification_agent_reply, is_conflict_retry,
    is_substantive_agent_reply, task_reply_claim_id,
};
pub use reply_claim::TaskReplyLock;
use reply_claim::current_reply_claim_id;

#[cfg(test)]
#[path = "retry/tests.rs"]
mod tests;

impl TaskChats {
    /// Hold the shared chat lock across a task reply's final planning write.
    pub fn lock_latest_user_message(
        slug: &str,
        key: &str,
        user_id: &str,
        reply_claim_id: Option<&str>,
    ) -> Result<Option<TaskReplyLock>, String> {
        let lock = Self::lock_store(slug)?;
        let (mut stored, _) = super::read_stored(slug)?;
        super::canonicalize(&mut stored);
        let Some(messages) = stored_history(&stored, key) else {
            return Ok(None);
        };
        let latest_user = messages
            .iter()
            .rposition(|message| message.role == ChatRole::User && !message.clarification);
        let Some(latest_user) = latest_user.filter(|index| messages[*index].id == user_id) else {
            return Ok(None);
        };
        let after = &messages[latest_user + 1..];
        if reply_claim_id.is_some() {
            if current_reply_claim_id(messages, user_id) != reply_claim_id {
                return Ok(None);
            }
            let Some(claim) = messages[latest_user + 1..]
                .iter()
                .find(|message| Some(message.id.as_str()) == reply_claim_id)
            else {
                return Ok(None);
            };
            if !claim_is_fresh(&claim.ts) {
                return Ok(None);
            }
        } else if after.iter().any(is_conflict_retry)
            || current_retry_claim_id(messages, user_id).is_some()
            || task_reply_claim_id(messages, user_id).is_some()
        {
            return Ok(None);
        }
        Ok(Some(TaskReplyLock {
            _lock: lock,
            slug: slug.to_owned(),
            key: key.to_owned(),
            user_id: user_id.to_owned(),
            reply_claim_id: reply_claim_id.map(str::to_owned),
        }))
    }
}

pub(super) fn has_task_reply_outcome(messages: &[ChatMessage]) -> bool {
    messages.iter().any(|message| {
        message.role == ChatRole::System
            && (message.id.starts_with(TASK_REPLY_OUTCOME_PREFIX)
                || message.id.starts_with(PLANNING_CONFLICT_APPLIED_PREFIX))
    })
}

pub(super) fn has_failed_task_reply(messages: &[ChatMessage]) -> bool {
    let Some(latest_user) = messages
        .iter()
        .rposition(|message| message.role == ChatRole::User && !message.clarification)
    else {
        return false;
    };
    messages[latest_user + 1..].iter().any(|message| {
        message.role == ChatRole::System
            && (message.id.starts_with(TASK_REPLY_FAILED_PREFIX)
                || message.id.starts_with(TASK_REPLY_EXHAUSTED_PREFIX))
    })
}

pub(super) fn has_active_reply_claim(messages: &[ChatMessage]) -> bool {
    if has_active_retry_claim(messages) {
        return true;
    }
    let Some(user_id) = latest_user_id(messages) else {
        return false;
    };
    let Some(claim_id) = task_reply_claim_id(messages, user_id) else {
        return false;
    };
    messages
        .iter()
        .find(|message| message.id == claim_id)
        .is_some_and(|claim| claim_is_fresh(&claim.ts))
}

fn latest_user_id(messages: &[ChatMessage]) -> Option<&str> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == ChatRole::User && !message.clarification)
        .map(|message| message.id.as_str())
}

pub(super) fn stored_history<'a>(
    stored: &'a super::StoredHistories,
    key: &str,
) -> Option<&'a Vec<ChatMessage>> {
    if let Some(uid) = stored.task_identities.get(key) {
        stored.task_histories.get(uid)
    } else if let Some(uid) = key.strip_prefix("@task:") {
        stored.task_histories.get(uid)
    } else if let Some(uid) = key.strip_prefix("@unlinked:") {
        stored.task_histories.get(uid)
    } else {
        stored.other_histories.get(key)
    }
}

fn current_retry_claim_id<'a>(messages: &'a [ChatMessage], user_id: &str) -> Option<&'a str> {
    let latest_user = messages
        .iter()
        .rposition(|message| message.role == ChatRole::User && !message.clarification)?;
    if messages[latest_user].id != user_id {
        return None;
    }
    let after = &messages[latest_user + 1..];
    if after.iter().any(is_substantive_agent_reply) {
        return None;
    }
    let retries = after
        .iter()
        .filter(|message| is_conflict_retry(message))
        .count();
    let last_conflict = after.iter().rposition(is_conflict_retry)?;
    let scope = claim_scope(user_id, retries);
    let trailing = &after[last_conflict + 1..];
    if trailing.iter().any(|message| {
        !(message.role == ChatRole::System && message.id.starts_with(&scope))
            && !(message.role == ChatRole::User && message.clarification)
            && !is_clarification_agent_reply(message)
    }) {
        return None;
    }
    trailing
        .iter()
        .rev()
        .find(|message| message.role == ChatRole::System && message.id.starts_with(&scope))
        .map(|message| message.id.as_str())
}

pub(super) fn has_active_retry_claim(messages: &[ChatMessage]) -> bool {
    let Some(latest_user) = messages
        .iter()
        .rposition(|message| message.role == ChatRole::User && !message.clarification)
    else {
        return false;
    };
    let after = &messages[latest_user + 1..];
    if after.iter().any(is_substantive_agent_reply) {
        return false;
    }
    let retries = after
        .iter()
        .filter(|message| is_conflict_retry(message))
        .count();
    let Some(last_conflict) = after.iter().rposition(is_conflict_retry) else {
        return false;
    };
    let scope = claim_scope(&messages[latest_user].id, retries);
    let trailing = &after[last_conflict + 1..];
    let claims = trailing
        .iter()
        .filter(|message| message.role == ChatRole::System && message.id.starts_with(&scope))
        .collect::<Vec<_>>();
    trailing.iter().all(|message| {
        (message.role == ChatRole::System && message.id.starts_with(&scope))
            || (message.role == ChatRole::User && message.clarification)
            || is_clarification_agent_reply(message)
    }) && claims.len() < MAX_PLANNING_CONFLICT_CLAIMS_PER_RETRY
        && claims.last().is_some_and(|claim| claim_is_fresh(&claim.ts))
}
