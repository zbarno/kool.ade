//! Save an explicit implementation decision without launching a planner turn.
use super::{ChatMessage, ChatRole, KooladeApp, Screen};
use sha2::Digest as _;

pub(super) fn task_context(
    planning: &crate::artifacts::planning_store::PlanningStore,
    document: &crate::artifacts::task_docs::TaskDocument,
) -> anyhow::Result<Option<String>> {
    let Some(metadata) = document.metadata.as_ref() else {
        return Ok(None);
    };
    let Some(task_intent_uid) = metadata.task_intent_uid.as_deref() else {
        return Ok(None);
    };
    let task = crate::core::lifecycle::LifecycleStore::new(planning).load_task(&metadata.uid)?;
    let work = crate::core::planning_work::load(planning)?;
    let root = work
        .iter()
        .find(|item| item.uid == task_intent_uid)
        .ok_or_else(|| anyhow::anyhow!("TaskIntent decision owner is missing"))?;
    anyhow::ensure!(
        root.feature_uid
            .as_deref()
            .is_none_or(|uid| uid == task.feature_uid),
        "Task decision feature identity does not match its TaskIntent"
    );
    if root.task_decisions.iter().any(|decision| {
        decision.task_uid == task.task_uid
            && decision.status == crate::core::planning_work::TaskDecisionStatusV1::Active
    }) {
        anyhow::ensure!(
            root.task_intent_cycle_uid.as_deref() == metadata.task_intent_cycle_uid.as_deref(),
            "Task decisions are stale for the current TaskIntent cycle"
        );
    }
    crate::core::planning_work::task_decision_context(
        &root.task_decisions,
        &task.task_uid,
        task_intent_uid,
        metadata.task_intent_cycle_uid.as_deref(),
        &task.feature_uid,
        task.task_spec_sha256.as_deref().unwrap_or_default(),
        &task.approved_plan_sha256,
    )
}

#[cfg(test)]
pub(super) fn latest_context(messages: &[ChatMessage], user_name: &str) -> Option<String> {
    let epoch = messages
        .iter()
        .rposition(|message| {
            message.role == ChatRole::Agent
                && (message.id.starts_with("needs-attention:")
                    || message.text.starts_with("## What Happened"))
        })
        .map_or(0, |index| index + 1);
    let messages = &messages[epoch..];
    let explicit = messages.iter().rev().find(|message| {
        message.role == ChatRole::User
            && !message.clarification
            && message.text.starts_with("I choose option (")
    });
    explicit
        .or_else(|| {
            messages.iter().rev().find(|m| {
                m.role == ChatRole::User
                    && !m.clarification
                    && !m.text.starts_with("I need clarification:")
                    && !m.text.starts_with("I'm not sure")
                    && !m.text.trim_end().ends_with('?')
                    && m.text
                        != "I approve creating a pull request for the verified implementation."
                    && m.text != "I am requesting changes before approving a pull request."
            })
        })
        .map(|message| format_answer(message, user_name))
}

#[cfg(test)]
fn format_answer(message: &ChatMessage, user_name: &str) -> String {
    format!(
        "Submitted at {} by {}:\n{}",
        message.ts,
        if user_name.trim().is_empty() {
            "the current user"
        } else {
            user_name
        },
        message.text
    )
}

#[cfg(test)]
pub(super) fn latest_context_for(
    messages: &[ChatMessage],
    user_name: &str,
    context: Option<&str>,
) -> Option<String> {
    if let Some(context) = context {
        messages
            .iter()
            .rev()
            .find(|message| {
                message.role == ChatRole::User
                    && !message.clarification
                    && message.answer_context.as_deref() == Some(context)
            })
            .map(|message| format_answer(message, user_name))
            .or_else(|| {
                let epoch = messages.iter().rposition(|message| {
                    message.role == ChatRole::Agent && message.id.starts_with("needs-attention:")
                })?;
                if messages[epoch].id != context {
                    return None;
                }
                let legacy = messages[epoch + 1..]
                    .iter()
                    .filter(|message| message.answer_context.is_none())
                    .cloned()
                    .collect::<Vec<_>>();
                latest_context(&legacy, user_name)
            })
    } else {
        latest_context(messages, user_name)
    }
}

impl KooladeApp {
    pub(super) fn submit_implementation_decision(&mut self, key: &str) -> bool {
        let ambiguous_identity = match &mut self.screen {
            Screen::Connected(project) => project.task_conversation_identity_is_ambiguous(key),
            Screen::Welcome => false,
        };
        if ambiguous_identity {
            self.toasts.warning(
                "This board conversation has conflicting artifact identities. Your draft is preserved until Kool.ad/e repairs the board link.",
            );
            return false;
        }
        let Screen::Connected(project) = &mut self.screen else {
            return false;
        };
        let blocked = project.queue.blocked.contains_key(key)
            || project.implementation_states.get(key).is_some_and(|state| {
                state.status == crate::core::implementation::ImplementationStatus::Blocked
            });
        if project.task_turns.contains_key(key) || !blocked {
            return false;
        }
        let text = project
            .task_chats
            .drafts
            .get(key)
            .cloned()
            .unwrap_or_default();
        if text.trim().is_empty() {
            return false;
        }
        project.task_chats.ensure_loaded(&project.chat_slug);
        let mut message = ChatMessage::new(ChatRole::User, &text, Some(key.into()));
        let detail = project
            .queue
            .blocked
            .get(key)
            .map(|failure| failure.message.as_str())
            .or_else(|| {
                project
                    .implementation_states
                    .get(key)
                    .map(|state| state.detail.as_str())
            });
        message.answer_context = detail.map(|detail| {
            crate::core::attention::answer_context(&project.state.repo_root, key, detail)
        });
        let Some(metadata) = project
            .task_documents
            .iter()
            .find(|document| document.path == key)
            .and_then(|document| document.metadata.as_ref())
        else {
            if let Err(error) = project
                .task_chats
                .append(&project.chat_slug, key, vec![message])
            {
                self.toasts.warning(error);
                return false;
            }
            self.toasts.warning(
                "Saved this direction to task chat, but it will not guide a worker until task identity is repaired.",
            );
            return false;
        };
        let (Some(task_intent_uid), Some(source_context)) = (
            metadata.task_intent_uid.as_deref(),
            message.answer_context.as_deref(),
        ) else {
            if let Err(error) = project
                .task_chats
                .append(&project.chat_slug, key, vec![message])
            {
                self.toasts.warning(error);
                return false;
            }
            self.toasts.warning(
                "Saved this direction to task chat, but it will not guide a worker without a current TaskIntent and blocker scope.",
            );
            return false;
        };
        if text.len() > crate::core::planning_work::MAX_TASK_DECISION_BYTES {
            self.toasts
                .warning("This implementation direction is too large to save safely.");
            return false;
        }
        let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
        let task = match lifecycle.load_task(&metadata.uid) {
            Ok(task) => task,
            Err(error) => {
                self.toasts
                    .warning(format!("Cannot validate this task direction: {error}"));
                return false;
            }
        };
        let Some(task_spec_sha256) = task.task_spec_sha256.as_deref() else {
            self.toasts
                .warning("Cannot save a task direction before the task contract is validated.");
            return false;
        };
        let created_at_unix_ms = chrono::Utc::now().timestamp_millis().max(1) as u64;
        let decision = crate::core::planning_work::TaskDecisionRecordV1 {
            schema_version: 1,
            decision_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            task_uid: task.task_uid.clone(),
            task_intent_uid: task_intent_uid.to_owned(),
            task_intent_cycle_uid: metadata.task_intent_cycle_uid.clone(),
            feature_uid: task.feature_uid.clone(),
            task_spec_sha256: task_spec_sha256.to_owned(),
            approved_plan_sha256: task.approved_plan_sha256.clone(),
            source_message_id: message.id.clone(),
            source_context_sha256: format!("{:x}", sha2::Sha256::digest(source_context.as_bytes())),
            source:
                crate::core::planning_work::TaskDecisionSourceV1::ExplicitImplementationDirection,
            authority: crate::core::lifecycle::GateActor::User,
            authorization_receipt: crate::core::planning_work::TaskDecisionAuthorityReceiptV1 {
                schema_version: 1,
                receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
                task_uid: task.task_uid.clone(),
                task_intent_uid: task_intent_uid.to_owned(),
                feature_uid: task.feature_uid.clone(),
                task_spec_sha256: task_spec_sha256.to_owned(),
                approved_plan_sha256: task.approved_plan_sha256.clone(),
                source_context_sha256: format!(
                    "{:x}",
                    sha2::Sha256::digest(source_context.as_bytes())
                ),
                source_message_id: message.id.clone(),
                subject_sha256: format!("{:x}", sha2::Sha256::digest(text.as_bytes())),
                recorded_at_unix_ms: created_at_unix_ms,
            },
            affected_requirement_ids: task.approved_requirement_ids.clone(),
            created_at_unix_ms,
            created_revision: 0,
            status: crate::core::planning_work::TaskDecisionStatusV1::Active,
            text: text.clone(),
        };
        if let Err(error) = project
            .task_chats
            .append(&project.chat_slug, key, vec![message])
        {
            self.toasts.warning(error);
            return false;
        }
        if let Err(error) = crate::core::planning_work::save_task_decision(
            &project.state.planning_store,
            task_intent_uid,
            decision,
        ) {
            self.toasts.warning(format!(
                "Cannot persist this implementation direction: {error:#}"
            ));
            return false;
        }
        project.task_chats.drafts.remove(key);
        project.activity.pending.push(format!(
            "User supplied an implementation decision for {key}: {}",
            crate::core::context_build::clip(&text, 1200)
        ));
        true
    }
}

#[cfg(test)]
mod tests;
