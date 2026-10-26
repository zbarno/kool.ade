use crate::{
    domain::{ChatMessage, ChatRole},
    harness::{
        BootstrapConversationResponseV1, BootstrapIntentSuggestionV1, HarnessRouteProvenance,
        PlanningRequest,
    },
    persistence::bootstrap_conversations::BootstrapConversationStore,
    ui::planning_board::{BootstrapConversationView, BootstrapSuggestionView},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::AtomicBool, mpsc},
};

mod turn;

#[derive(Default)]
pub(crate) struct Controller {
    conversations: BTreeMap<String, Conversation>,
    active: BTreeMap<String, ActiveTurn>,
    next_generation: u64,
}

#[derive(Default)]
struct Conversation {
    loaded: bool,
    messages: Vec<ChatMessage>,
    draft: String,
    error: Option<String>,
    suggestion: Option<SavedSuggestion>,
}

pub(super) struct SavedSuggestion {
    pub(super) value: BootstrapIntentSuggestionV1,
    pub(super) generation: u64,
    pub(super) task_fingerprint: String,
    pub(super) route: HarnessRouteProvenance,
    pub(super) request: PlanningRequest,
}

struct ActiveTurn {
    task_id: String,
    generation: u64,
    task_fingerprint: String,
    route: HarnessRouteProvenance,
    request: PlanningRequest,
    cancel: Arc<AtomicBool>,
    result: mpsc::Receiver<Result<BootstrapConversationResponseV1, String>>,
}

pub(super) struct CompletedTurn {
    active: ActiveTurn,
    result: Result<BootstrapConversationResponseV1, String>,
}

impl CompletedTurn {
    pub(in crate::app::root::bootstrap_conversation) fn task_id(&self) -> &str {
        &self.active.task_id
    }
}

impl Controller {
    fn ensure_loaded(&mut self, task_id: &str) {
        if !task_id.starts_with("setup:") {
            return;
        }
        let conversation = self.conversations.entry(task_id.to_owned()).or_default();
        if conversation.loaded {
            return;
        }
        conversation.loaded = true;
        match BootstrapConversationStore::load(task_id) {
            Ok(messages) => conversation.messages = messages,
            Err(_) => {
                conversation.error =
                    Some("The machine-local setup conversation could not be loaded safely.".into());
                return;
            }
        }
        if conversation
            .messages
            .last()
            .is_some_and(|message| message.role == ChatRole::User)
        {
            let notice = ChatMessage::new(
                ChatRole::System,
                "The previous setup-assistant response did not finish. Send a new message when ready; it will not resume automatically.",
                None,
            );
            match BootstrapConversationStore::append(task_id, &[notice]) {
                Ok(messages) => conversation.messages = messages,
                Err(_) => {
                    conversation.error = Some(
                        "The previous setup-assistant response did not finish. Conversation history is read-only until local storage is available.".into(),
                    );
                }
            }
        }
    }

    pub(crate) fn view(&mut self, task_id: &str) -> BootstrapConversationView {
        self.ensure_loaded(task_id);
        let conversation = self.conversations.entry(task_id.to_owned()).or_default();
        let active_for_task = self.active.contains_key(task_id);
        BootstrapConversationView {
            messages: conversation.messages.clone(),
            busy: active_for_task,
            active_for_task,
            error: conversation.error.clone(),
            suggestion: conversation.suggestion.as_ref().map(|suggestion| {
                BootstrapSuggestionView {
                    operation: super::suggestion_view(&suggestion.value),
                    turn_generation: suggestion.generation,
                }
            }),
        }
    }

    pub(crate) fn draft_mut(&mut self, task_id: &str) -> Option<&mut String> {
        if !task_id.starts_with("setup:") {
            return None;
        }
        self.ensure_loaded(task_id);
        Some(
            &mut self
                .conversations
                .entry(task_id.to_owned())
                .or_default()
                .draft,
        )
    }

    pub(super) fn set_error(&mut self, task_id: &str, error: String) {
        self.ensure_loaded(task_id);
        self.conversations
            .entry(task_id.to_owned())
            .or_default()
            .error = Some(error);
    }

    fn append_system(&mut self, task_id: &str, text: &str) {
        let message = ChatMessage::new(ChatRole::System, text, None);
        match BootstrapConversationStore::append(task_id, &[message]) {
            Ok(messages) => {
                let conversation = self.conversations.entry(task_id.to_owned()).or_default();
                conversation.loaded = true;
                conversation.messages = messages;
            }
            Err(_) => self.set_error(
                task_id,
                "A setup-assistant status note could not be saved locally.".into(),
            ),
        }
    }
}
