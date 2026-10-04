//! Human-facing text for saved agent protocol responses. Never rewrites history.
use crate::domain::{ChatMessage, ChatRole};

pub fn readable(message: &ChatMessage) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    if message.role == ChatRole::System
        && message
            .text
            .starts_with("⚠ Turn rejected — nothing was written.")
    {
        return Cow::Borrowed("This update could not be saved. Nothing changed. Please try again.");
    }
    if message.role != ChatRole::Agent {
        return Cow::Borrowed(&message.text);
    }
    let raw = crate::harness::pi_extract::extract_json_object(&message.text);
    if let Some(value) = raw
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
    {
        let protocol = [
            "assistant_message",
            "assistantMessage",
            "schema_version",
            "schemaVersion",
            "open_items_updated",
            "openItemsUpdated",
            "document_updates",
            "documentUpdates",
        ]
        .iter()
        .any(|key| value.get(key).is_some());
        if protocol {
            if let Some(reply) = value
                .get("assistant_message")
                .or_else(|| value.get("assistantMessage"))
                .and_then(serde_json::Value::as_str)
                .filter(|text| !text.trim().is_empty())
            {
                return Cow::Owned(reply.trim().to_string());
            }
            return Cow::Borrowed("The agent returned no readable reply. Please try again.");
        }
    }
    // Broken/truncated envelopes must not spill implementation fields into chat.
    if message.text.contains('{')
        && [
            "\"assistant_message\"",
            "\"assistantMessage\"",
            "\"schema_version\"",
            "\"schemaVersion\"",
        ]
        .iter()
        .any(|key| message.text.contains(key))
    {
        return Cow::Borrowed("The agent returned an unreadable reply. Please try again.");
    }
    Cow::Borrowed(&message.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_protocol_replies_without_losing_user_json_or_plain_agent_examples() {
        for body in [
            r#"{"assistant_message":"Use corporate SSO.","open_items_updated":[]}"#,
            "Here is the result:\n```json\n{\"assistantMessage\":\"Use corporate SSO.\",\"schemaVersion\":1}\n```",
        ] {
            let message = ChatMessage::new(ChatRole::Agent, body, None);
            assert_eq!(readable(&message), "Use corporate SSO.");
            assert_eq!(message.text, body, "raw diagnostics remain intact");
            let user = ChatMessage::new(ChatRole::User, body, None);
            assert_eq!(readable(&user), body);
        }
        let example = ChatMessage::new(
            ChatRole::Agent,
            "Example:\n```json\n{\"provider\":\"SSO\"}\n```",
            None,
        );
        assert_eq!(readable(&example), example.text);
        let broken = ChatMessage::new(ChatRole::Agent, "{\"assistant_message\":\"unfinished", None);
        assert_eq!(
            readable(&broken),
            "The agent returned an unreadable reply. Please try again."
        );
    }
}
