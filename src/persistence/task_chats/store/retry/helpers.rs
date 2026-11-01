use super::*;

pub(super) fn is_conflict_retry(message: &ChatMessage) -> bool {
    message.role == ChatRole::System && message.id.starts_with(PLANNING_CONFLICT_RETRY_PREFIX)
}

pub(super) fn is_substantive_agent_reply(message: &ChatMessage) -> bool {
    message.role == ChatRole::Agent && !message.clarification
}

pub(super) fn is_clarification_agent_reply(message: &ChatMessage) -> bool {
    message.role == ChatRole::Agent && message.clarification
}

pub(super) fn claim_scope(user_id: &str, retries: usize) -> String {
    format!("{PLANNING_CONFLICT_CLAIM_PREFIX}{user_id}:{retries}:")
}

pub(super) fn claim_is_fresh(timestamp: &chrono::DateTime<chrono::Utc>) -> bool {
    let lease = chrono::Duration::from_std(TASK_REPLY_CLAIM_LEASE)
        .expect("the retry lease fits in chrono duration");
    chrono::Utc::now() < *timestamp + lease
}

pub(super) fn task_reply_claim_id<'a>(
    messages: &'a [ChatMessage],
    user_id: &str,
) -> Option<&'a str> {
    let latest_user = messages
        .iter()
        .rposition(|message| message.role == ChatRole::User && !message.clarification)?;
    if messages[latest_user].id != user_id {
        return None;
    }
    let after = &messages[latest_user + 1..];
    if after.iter().any(is_substantive_agent_reply)
        || after.iter().any(is_conflict_retry)
        || super::has_task_reply_outcome(after)
    {
        return None;
    }
    let prefix = format!("{TASK_REPLY_CLAIM_PREFIX}{user_id}:");
    after
        .iter()
        .rev()
        .find(|message| message.role == ChatRole::System && message.id.starts_with(&prefix))
        .map(|message| message.id.as_str())
}
