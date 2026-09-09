//! The connected-project session: loaded artifacts, chat (persisted OUTSIDE
//! git under ~/.packet), running-turn bookkeeping, and refresh cadence.

use std::sync::Arc;

use crate::core::gitops::{self, GitSnapshot};
use crate::core::state::PlannerState;
use crate::core::turn::TurnController;
use crate::domain::chatlog::ChatMessage;
use crate::domain::user::CurrentUser;
use crate::persistence::chat_store;

pub struct Project {
    pub state: PlannerState,
    /// Project slug keying the ~/.packet chat store.
    pub chat_slug: String,
    pub chat: Vec<ChatMessage>,
    pub draft: String,
    pub active_turn: Option<Arc<TurnController>>,
    pub live_progress: crate::harness::LiveProgress,
    /// Which item the app decided to press the user with (routing verdict).
    pub next_question_id: Option<String>,
    pub git: GitSnapshot,
    pub task_documents: Vec<crate::artifacts::task_docs::TaskDocument>,
}

impl Project {
    /// Snapshot the most recent chat for the turn's prompt context.
    pub fn recent_chat_tuples(&self, max_msgs: usize, clip_chars: usize) -> Vec<(String, String)> {
        let start = self.chat.len().saturating_sub(max_msgs);
        self.chat[start..]
            .iter()
            .map(|m| {
                let who = match m.role {
                    crate::domain::chatlog::ChatRole::User => "you".to_string(),
                    crate::domain::chatlog::ChatRole::Agent => "planner".to_string(),
                    crate::domain::chatlog::ChatRole::System => "system".to_string(),
                };
                (who, clip(&m.text, clip_chars))
            })
            .collect()
    }

    /// Push messages into memory AND the durable ~/.packet store.
    pub fn remember_chat(&mut self, msgs: Vec<ChatMessage>) {
        if msgs.is_empty() {
            return;
        }
        let _ = chat_store::append(&self.chat_slug, &msgs);
        self.chat.extend(msgs);
    }

    pub fn refresh_git(&mut self) {
        self.git = gitops::snapshot(&self.state.repo_root);
    }
}

/// Hydrate display chat from ~/.packet (tolerant of corruption by design).
pub fn load_chat(slug: &str) -> Vec<ChatMessage> {
    chat_store::load(slug).0
}

fn clip(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

/// Seed a helpful first line for freshly-connected projects.
pub fn welcome_message(project_title: &str) -> ChatMessage {
    let text = format!(
        "Connected to “{project_title}”. Describe your idea, paste requirements, \
         or ask me to draft the initial specification. I keep the spec and the \
         open-items queue in git and only ever talk to you through this chat."
    );
    crate::domain::chatlog::ChatMessage::new(crate::domain::chatlog::ChatRole::System, text, None)
}

/// Convert the stored user into the routing identity, falling back gently.
pub fn routing_user(config_user: Option<&CurrentUser>, project_name: &str) -> CurrentUser {
    match config_user {
        Some(u) if u.is_set() => u.clone(),
        _ => CurrentUser::new(project_name, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_bounds_hold() {
        assert_eq!(clip("abc", 10), "abc");
        assert_eq!(clip("abcdefghij", 4), "abcd…");
    }
}
