use super::*;
use chat::remember_turn_messages;

impl KooladeApp {
    pub(super) fn adopt_rejected_turn(
        &mut self,
        project: &mut Project,
        problems: Vec<String>,
        final_text: String,
        conversation: Option<&str>,
    ) {
        let mut chat = vec![ChatMessage::new(
            ChatRole::System,
            format!(
                "⚠ Turn rejected — nothing was written.\n{}",
                problems.join("\n")
            ),
            None,
        )];
        chat.push(ChatMessage::new(
            ChatRole::Agent,
            if final_text.trim().is_empty() {
                "(no legible reply — try again)".to_string()
            } else {
                final_text
            },
            None,
        ));
        remember_turn_messages(project, conversation, chat);
        self.toasts.danger(format!(
            "Rejected: {}",
            problems.first().map(String::as_str).unwrap_or("")
        ));
    }

    pub(super) fn adopt_failed_turn(
        &mut self,
        project: &mut Project,
        error: crate::error::AppError,
        conversation: Option<&str>,
    ) {
        remember_turn_messages(
            project,
            conversation,
            vec![ChatMessage::new(
                ChatRole::System,
                match &error {
                    crate::error::AppError::InvalidResponse { .. } => {
                        format!("Task generation needs attention: {}", error.detail())
                    }
                    _ => format!("Planning stopped: {}", error.headline()),
                },
                None,
            )],
        );
        self.toasts.danger(error.headline());
    }
}
