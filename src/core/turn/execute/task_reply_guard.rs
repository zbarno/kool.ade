use super::*;

pub(super) fn lock_latest_reply(
    slug: &str,
    key: &str,
    message_id: &str,
    reply_claim_id: Option<&str>,
    final_text: &str,
    elapsed: Duration,
) -> Result<crate::persistence::task_chats::TaskReplyLock, Box<TurnOutcome>> {
    match crate::persistence::task_chats::TaskChats::lock_latest_user_message(
        slug, key, message_id, reply_claim_id,
    ) {
        Ok(Some(lock)) => Ok(lock),
        Ok(None) => Err(Box::new(TurnOutcome::Rejected {
            problems: vec!["This task reply was superseded by a newer saved answer; no planning changes were applied.".into()],
            final_text: final_text.to_owned(),
            elapsed,
        })),
        Err(error) => Err(Box::new(TurnOutcome::Rejected {
            problems: vec![format!("Planning files changed on disk while verifying the saved task reply: {error}. The saved reply remains available for an internal retry.")],
            final_text: final_text.to_owned(),
            elapsed,
        })),
    }
}
