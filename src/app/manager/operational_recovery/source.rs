use super::*;
pub(super) fn source(project: &Project, key: &str) -> Option<Work> {
    project
        .planning_work
        .iter()
        .find(|w| w.key == key)
        .cloned()
        .or_else(|| {
            let item = project
                .state
                .items
                .iter()
                .chain(&project.state.resolved_items)
                .find(|i| i.conversation_key() == key)?;
            let mut work = Work::new(
                key.into(),
                item.question.clone(),
                item.question.clone(),
                item.reason.clone(),
            );
            work.uid = item.uid.clone()?;
            work.feature_id = item.feature_id.clone();
            work.feature_uid = item.feature_uid.clone();
            work.parent_uid = Some(work.uid.clone());
            work.task_intent_uid = item.task_intent_uid.clone().or_else(|| {
                work.feature_uid
                    .as_deref()
                    .and_then(|uid| Work::task_intent_uid_for_feature(&project.planning_work, uid))
            });
            work.task_intent_cycle_uid = item.task_intent_cycle_uid.clone();
            Some(work)
        })
}

pub(super) fn failure_candidates(project: &Project) -> Vec<Work> {
    project
        .task_chats
        .messages
        .iter()
        .filter_map(|(key, messages)| {
            if !project.task_chats.has_failed_task_reply(key)
                || project.task_turns.contains_key(key)
                || project.task_chats.has_active_reply_claim(key)
                || project.task_chats.has_pending_user_reply(key)
            {
                return None;
            }
            let user = messages
                .iter()
                .rev()
                .find(|m| m.role == ChatRole::User && !m.clarification)?;
            let source = source(project, key)?;
            if project
                .cancelled_work
                .contains(&crate::persistence::cancelled_work::planning_id(
                    &source.uid,
                ))
            {
                return None;
            }
            let evidence = messages
                .iter()
                .rev()
                .find(|m| m.role == ChatRole::System)?
                .text
                .clone();
            let failure = if evidence.contains("feature")
                && (evidence.contains("identity") || evidence.contains("replaced"))
            {
                FailureKind::FeatureIdentity
            } else if evidence.contains("planning") && evidence.contains("conflict") {
                FailureKind::PlanningConflict
            } else if evidence.contains("format") || evidence.contains("JSON") {
                FailureKind::ResponseFormat
            } else {
                FailureKind::Execution
            };
            Some(operational_recovery::task(
                &source, &user.id, failure, evidence,
            ))
        })
        .collect::<Vec<_>>()
}
