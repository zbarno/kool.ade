use super::*;

const RENEW_INTERVAL: Duration = Duration::from_secs(5 * 60);

pub(super) fn renew(project: &mut crate::app::session::Project) {
    let due = project
        .task_turn_reply_claims
        .iter()
        .filter(|(key, (_, renewed_at))| {
            renewed_at.elapsed() >= RENEW_INTERVAL && project.task_turns.contains_key(*key)
        })
        .filter_map(|(key, (claim_id, _))| {
            project
                .task_turn_user_message_ids
                .get(key)
                .map(|user_id| (key.clone(), claim_id.clone(), user_id.clone()))
        })
        .collect::<Vec<_>>();
    for (key, claim_id, user_id) in due {
        let result = project.task_chats.renew_task_reply_claim(
            &project.chat_slug,
            &key,
            &user_id,
            &claim_id,
        );
        if let Some((_, renewed_at)) = project.task_turn_reply_claims.get_mut(&key) {
            *renewed_at = std::time::Instant::now();
        }
        match result {
            Ok(true) => {}
            Ok(false) => {
                if let Some(turn) = project.task_turns.get(&key) {
                    turn.request_cancel();
                }
            }
            Err(error) => {
                project.queue.last_error =
                    format!("Could not renew the saved task reply retry claim: {error}");
            }
        }
    }
}
