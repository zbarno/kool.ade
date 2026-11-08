use super::super::{KooladeApp, Screen};

impl KooladeApp {
    pub(super) fn retry_manager_check_for_surface(&mut self) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.activity.manager.is_some() {
            return;
        }
        project
            .activity
            .pending
            .push("The user requested a Manager board check from Project activity.".into());
        project.activity.last_manager_error = None;
        project.activity.last_update = None;
    }
}

macro_rules! manager_activity_surface_methods {
    () => {
        fn specification_chat_messages(&self) -> Vec<crate::domain::ChatMessage> {
            match &self.screen {
                Screen::Connected(project) => project
                    .chat
                    .iter()
                    .filter(|message| message.specification_context.is_some())
                    .cloned()
                    .collect(),
                Screen::Welcome => Vec::new(),
            }
        }

        fn legacy_chat_messages(&self) -> Vec<crate::domain::ChatMessage> {
            match &self.screen {
                Screen::Connected(project) => project
                    .chat
                    .iter()
                    .filter(|message| message.specification_context.is_none())
                    .cloned()
                    .collect(),
                Screen::Welcome => Vec::new(),
            }
        }

        fn manager_activity(&self) -> &[crate::domain::ChatMessage] {
            match &self.screen {
                Screen::Connected(project) => &project.activity.manager_history,
                Screen::Welcome => &[],
            }
        }

        fn reviewed_baseline_evidence(&self, review_id: &str) -> Vec<crate::domain::chatlog::VerifiedRepositoryEvidence> {
            self.manager_activity().iter()
                .find(|message| message.role == crate::domain::ChatRole::Agent && message.baseline_review_id.as_deref() == Some(review_id))
                .map(|message| message.verified_repository_evidence.iter().take(8).cloned().collect())
                .unwrap_or_default()
        }

        fn manager_running(&self) -> bool {
            matches!(&self.screen, Screen::Connected(project) if project.activity.manager.is_some())
        }

        fn manager_progress(&self) -> Option<&crate::harness::LiveProgress> {
            match &self.screen {
                Screen::Connected(project) if project.activity.manager.is_some() => {
                    Some(&project.activity.manager_progress)
                }
                _ => None,
            }
        }

        fn manager_history_error(&self) -> Option<&str> {
            match &self.screen {
                Screen::Connected(project) => project.activity.manager_history_error.as_deref(),
                Screen::Welcome => None,
            }
        }

        fn manager_last_error(&self) -> Option<&str> {
            match &self.screen {
                Screen::Connected(project) => project.activity.last_manager_error.as_deref(),
                Screen::Welcome => None,
            }
        }

        fn retry_manager_check(&mut self) {
            self.retry_manager_check_for_surface();
        }

        fn specification_chat_busy(&self) -> bool {
            matches!(&self.screen, Screen::Connected(project)
                if project.active_turn.as_ref().is_some_and(|turn| turn.specification_chat.is_some()))
        }

        fn specification_chat_progress(&self) -> Option<&crate::harness::LiveProgress> {
            match &self.screen {
                Screen::Connected(project)
                    if project.active_turn.as_ref().is_some_and(|turn| turn.specification_chat.is_some()) =>
                {
                    Some(&project.live_progress)
                }
                _ => None,
            }
        }
    };
}

pub(super) use manager_activity_surface_methods;
