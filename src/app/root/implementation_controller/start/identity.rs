use super::*;

impl KooladeApp {
    pub(super) fn reject_ambiguous_task_start(&mut self, ticket: &str, manual: bool) -> bool {
        let ambiguous = match &mut self.screen {
            Screen::Connected(project) => project.task_conversation_identity_is_ambiguous(ticket),
            Screen::Welcome => false,
        };
        if !ambiguous {
            return false;
        }
        let message = "Implementation is blocked because this board task has conflicting artifact identities. Repair the board link before starting work.";
        if let Screen::Connected(project) = &mut self.screen {
            project.queue.last_error = message.to_owned();
        }
        if manual {
            self.toasts.warning(message);
        }
        true
    }
}
