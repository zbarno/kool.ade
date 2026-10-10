//! The connected-project session: loaded artifacts, chat (persisted OUTSIDE
//! git under ~/.koolade), running-turn bookkeeping, and refresh cadence.

use std::rc::Rc;

use crate::core::gitops::GitSnapshot;
use crate::core::state::PlannerState;
use crate::core::turn::TurnController;
use crate::domain::chatlog::ChatMessage;
use crate::persistence::chat_store;

mod cancellation;
mod implementations;
mod task_status;
#[cfg(test)]
pub(crate) mod test_support;

pub struct Project {
    pub task_chats: crate::persistence::task_chats::TaskChats,
    pub activity: super::manager::WorkspaceActivity,
    pub state: PlannerState,
    /// Project slug keying the ~/.koolade chat store.
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
    pub reconciliation: crate::app::reconciliation_lifecycle::Lifecycle,
    pub investigation: Option<crate::core::investigation::Controller>,
    pub investigation_attempted: std::collections::HashSet<String>,
    /// Suppresses agent-item investigation spawning until this instant; set
    /// after a drift deferral so a live project stops churning model calls.
    pub investigation_cooldown_until: Option<std::time::Instant>,
    pub last_pr_refresh: Option<std::time::Instant>,
    pub active_turn: Option<Rc<TurnController>>,
    pub task_turns: std::collections::BTreeMap<String, Rc<TurnController>>,
    pub task_live: std::collections::BTreeMap<String, crate::harness::LiveProgress>,
    pub planning_work: Vec<crate::core::planning_work::Work>,
    pub active_planning_work: Option<String>,
    pub live_progress: crate::harness::LiveProgress,
    /// Which item the app decided to press the user with (routing verdict).
    pub next_question_id: Option<String>,
    pub git: GitSnapshot,
    pub task_documents: Vec<crate::artifacts::task_docs::TaskDocument>,
    pub archived_tasks: std::collections::BTreeSet<String>,
    pub cancelled_work: std::collections::BTreeSet<String>,
}

impl Project {
    pub fn save_planning_work(&mut self) -> Result<(), String> {
        let _guard = crate::core::writer_gate::acquire();
        match crate::core::planning_work::save_expected(
            &self.state.planning_store,
            &self.planning_work,
            &self.state.baseline_planning_revision,
        ) {
            Ok(revision) => {
                self.state.baseline_planning_revision = revision;
                match crate::core::planning_work::load(&self.state.planning_store) {
                    Ok(work) => self.planning_work = work,
                    Err(error) => {
                        self.activity.pending_planning_work = true;
                        return Err(format!("Planning work saved but could not reload: {error}"));
                    }
                }
                self.activity.pending_planning_work = false;
                self.activity
                    .pending
                    .retain(|event| !event.starts_with("Planning work could not be saved:"));
                Ok(())
            }
            Err(error) => {
                self.activity.pending_planning_work = true;
                if !self
                    .activity
                    .pending
                    .iter()
                    .any(|event| event.starts_with("Planning work could not be saved:"))
                {
                    self.activity.pending.push(format!(
                        "Planning work could not be saved: {error}. Kool.ad/e will retry automatically."
                    ));
                }
                Err(error.to_string())
            }
        }
    }

    pub fn bind_task_conversation_identities(&mut self) {
        match self.queue.bind_task_documents(&self.task_documents) {
            Ok(true) => {
                if let Err(error) = self.queue.save(&self.state.repo_root) {
                    self.queue.last_error =
                        format!("Task identity migration could not be saved: {error}");
                }
            }
            Ok(false) => {}
            Err(error) => {
                self.queue.last_error =
                    format!("Task queue references could not be linked: {error}")
            }
        }
        if let Err(error) = self.task_chats.bind_task_documents(&self.task_documents) {
            self.task_chats.error =
                Some(format!("Task conversations could not be linked: {error}"));
        }
    }

    pub fn task_interaction_context(&self, focus: &str) -> String {
        let context = self.task_chats.project_context(focus, 24000);
        if context.is_empty() {
            return context;
        }
        format!(
            "{context}\nConversation storage status: {}\nComplete task conversation history: {}\n",
            self.task_chats.error.as_deref().unwrap_or("Saved"),
            crate::persistence::project_dir(&self.chat_slug)
                .join("task-conversations.json")
                .display()
        )
    }

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

    /// Push messages into memory AND the durable ~/.koolade store.
    pub fn remember_chat(&mut self, msgs: Vec<ChatMessage>) {
        if msgs.is_empty() {
            return;
        }
        let _ = chat_store::append(&self.chat_slug, &msgs);
        self.chat.extend(msgs);
    }

    pub fn remember_turn_chat(&mut self, msgs: Vec<ChatMessage>) {
        if let Some(key) = self.task_chats.active.clone() {
            self.activity.pending.push(format!(
                "Task conversation {key} updated: {}",
                msgs.iter()
                    .map(|m| format!(
                        "{:?}: {}",
                        m.role,
                        crate::core::context_build::clip(&m.text, 1600)
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
            self.task_chats
                .remember_response(&self.chat_slug, &key, msgs);
        } else {
            self.remember_chat(msgs);
        }
    }
}

/// Hydrate display chat from ~/.koolade (tolerant of corruption by design).
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
