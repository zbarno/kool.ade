//! Application-owned receipt for the exact question/choices a saved reply answers.
use crate::domain::{ChatMessage, ChatRole, item::OpenItem};
use sha2::{Digest, Sha256};

pub(crate) fn context(item: &OpenItem) -> String {
    let bytes = serde_json::to_vec(&(&item.uid, &item.question, &item.decision_brief))
        .expect("question data serializes");
    format!("planning-answer:{:x}", Sha256::digest(bytes))
}

pub(crate) fn is_saved(item: &OpenItem, messages: &[ChatMessage]) -> bool {
    let fingerprint = context(item);
    messages
        .iter()
        .rev()
        .find(|message| message.role == ChatRole::User && !message.clarification)
        .is_some_and(|message| {
            message.answer_context.as_deref() == Some(&fingerprint)
                || (message.answer_context.is_none()
                    && item.decision_brief.as_ref().is_some_and(|brief| {
                        brief.options.iter().any(|option| {
                            message.text.starts_with(&format!(
                                "I choose option ({}): {}",
                                option.id, option.label
                            ))
                        })
                    }))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_survives_escalation_but_a_changed_question_requires_a_new_answer() {
        let mut item = OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::High,
            crate::domain::ItemKind::Question,
            "General".into(),
            None,
            "Which store?".into(),
            "Need a decision".into(),
        );
        let mut user = ChatMessage::new(ChatRole::User, "Use SQLite", None);
        user.answer_context = Some(context(&item));
        item.authority = crate::domain::Authority::Review;
        item.evidence = "Choice recorded; review remains".into();
        assert!(is_saved(&item, &[user.clone()]));
        item.question = "Which durability mode?".into();
        assert!(!is_saved(&item, &[user]));
    }
}
