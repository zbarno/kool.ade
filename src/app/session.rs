//! The connected-project session: loaded artifacts, chat (persisted OUTSIDE
//! git under ~/.packet), running-turn bookkeeping, and refresh cadence.

use std::sync::Arc;

use crate::core::gitops::{self, GitSnapshot};
use crate::core::state::PlannerState;
use crate::core::turn::TurnController;
use crate::domain::chatlog::ChatMessage;
use crate::persistence::chat_store;

pub struct Project {
    pub task_chats: crate::persistence::task_chats::TaskChats,
    pub activity: super::manager::WorkspaceActivity,
    pub state: PlannerState,
    /// Project slug keying the ~/.packet chat store.
    pub chat_slug: String,
    pub chat: Vec<ChatMessage>,
    pub draft: String,
    pub queue: crate::core::implementation_queue::Queue,
    pub queue_lock: Option<std::fs::File>,
    pub active_implementations:
        std::collections::BTreeMap<String, crate::core::implementation::Controller>,
    pub implementation_states:
        std::collections::BTreeMap<String, crate::core::implementation::Implementation>,
    pub pr_refresh: Option<crate::core::implementation::PrRefresh>,
    pub reconciliation: Option<crate::core::reconciliation::Controller>,
    pub reconciliation_attempted: std::collections::HashSet<String>,
    pub reconciliation_error: Option<String>,
    /// Suppresses reconciliation spawning until this instant; set after a
    /// drift deferral so a live project stops churning model calls.
    pub reconciliation_cooldown_until: Option<std::time::Instant>,
    pub investigation: Option<crate::core::investigation::Controller>,
    pub investigation_attempted: std::collections::HashSet<String>,
    /// Suppresses agent-item investigation spawning until this instant; set
    /// after a drift deferral so a live project stops churning model calls.
    pub investigation_cooldown_until: Option<std::time::Instant>,
    pub last_pr_refresh: Option<std::time::Instant>,
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

    pub fn remember_turn_chat(&mut self, msgs: Vec<ChatMessage>) {
        if let Some(key) = self.task_chats.active.clone() {
            self.task_chats
                .remember_response(&self.chat_slug, &key, msgs);
        } else {
            self.remember_chat(msgs);
        }
    }

    pub fn refresh_implementations(&mut self) {
        let latest = crate::core::implementation::load_all(&self.state.repo_root)
            .into_iter()
            .map(|state| (state.ticket.clone(), state))
            .collect::<std::collections::BTreeMap<_, _>>();
        for (ticket, state) in &latest {
            if let Some(previous) = self.implementation_states.get(ticket) {
                if previous.status != state.status || previous.pr_state != state.pr_state {
                    self.activity.pending.push(format!(
                        "{ticket}: {} → {}; PR {:?}",
                        previous.status, state.status, state.pr_state
                    ));
                }
            }
        }
        self.implementation_states = latest;
        for ticket in self.implementation_states.keys() {
            if !self.activity.tasks.contains_key(ticket) {
                if let Some(activity) =
                    crate::core::implementation::load_activity(&self.state.repo_root, ticket)
                {
                    self.activity.tasks.insert(ticket.clone(), activity);
                }
            }
        }
    }

    pub fn save_task_activity(&mut self, ticket: &str) {
        if let Some(activity) = self.activity.tasks.get(ticket) {
            if let Err(error) =
                crate::core::implementation::save_activity(&self.state.repo_root, ticket, activity)
            {
                // Record the failure beside (not over) the last real status
                // so a terminal snapshot does not reduce to infra noise.
                const MARK: &str = "Activity could not be saved: ";
                let previous = self
                    .activity
                    .tasks
                    .get(ticket)
                    .and_then(|progress| progress.activity.clone())
                    .unwrap_or_default();
                // Keep exactly ONE annotation slot: collapse any earlier
                // failure note so repeated faults cannot stack diagnostics.
                let head = previous.split(MARK).next().unwrap_or("").trim_end_matches('\n');
                let rendered = if head.is_empty() {
                    format!("{MARK}{error}")
                } else {
                    format!("{head}\n{MARK}{error}")
                };
                self.activity.tasks.get_mut(ticket).unwrap().activity = Some(rendered);
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_bounds_hold() {
        assert_eq!(clip("abc", 10), "abc");
        assert_eq!(clip("abcdefghij", 4), "abcd…");
    }
}
