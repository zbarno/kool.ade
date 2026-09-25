//! Presentation layer (egui). Views draw exclusively through the
//! [`Surface`] trait, so the app state machine (`crate::app`) stays
//! decoupled from pixels.

pub mod chat_pane;
pub mod feature_approval;
pub mod items_pane;
pub mod layout;
pub mod markdown;
pub mod message_text;
pub mod overlays;
pub mod reply_tail;
pub mod spec_viewer;
pub mod task_activity;
pub mod task_chat;
pub mod theme;
pub mod toast;

pub use chat_pane::Intent;
pub use layout::HeaderAction;
pub use toast::ToastQueue;

use crate::domain::chatlog::ChatMessage;
use crate::domain::item::OpenItem;
use crate::domain::user::CurrentUser;

/// Everything the chrome/panes need: read-mostly accessors plus two command
/// sinks (composer intents, header actions).
pub trait Surface {
    // ------- header -------
    fn session_title(&self) -> &str;
    fn is_git_repo(&self) -> bool;
    fn git_branch(&self) -> &str;
    fn git_head(&self) -> &str;
    fn git_dirty(&self) -> bool;
    // ------- chat pane -------
    fn chat_messages(&self) -> &[ChatMessage];
    fn task_messages(&self, _key: &str) -> &[ChatMessage] {
        &[]
    }
    fn prepare_task_chat(&mut self, _key: &str) {}
    fn task_chat_context(&self, _key: &str) -> Option<String> { None }
    fn task_draft(&mut self, _key: &str) -> Option<&mut String> {
        None
    }
    fn send_task_reply(&mut self, _key: &str) {}
    fn send_implementation_decision(&mut self, key: &str) {
        self.send_task_reply(key);
    }
    fn task_chat_active(&self, _key: &str) -> bool {
        false
    }
    fn task_reply_progress(&self, _key: &str) -> Option<&crate::harness::LiveProgress> { None }
    fn task_reply_busy(&self) -> bool {
        self.conversation_busy()
    }
    fn cancel_task_reply(&mut self, _key: &str) {}
    fn task_chat_error(&self) -> Option<&str> {
        None
    }
    fn retry_task_chat_save(&mut self) {}
    /// Best-effort per-frame flush of unsaved task-conversation replies; a
    /// no-op unless a previous save failed, so a transient store hiccup does
    /// not strand the last remembered reply.
    fn drain_task_chat_saves(&mut self) {}
    fn chat_draft(&mut self) -> &mut String;
    fn is_busy(&self) -> bool;
    fn conversation_busy(&self) -> bool;
    fn task_progress(&self, ticket: &str) -> Option<&crate::harness::LiveProgress>;
    /// None selects the project-wide series. Values are observed updates / 10s.
    fn activity_samples(&self, key: Option<&str>) -> Vec<(i64, u64)> {
        key.and_then(|key| self.task_progress(key))
            .map(|p| p.telemetry.samples.clone())
            .unwrap_or_default()
    }
    fn activity_active(&self, key: &str) -> bool {
        self.implementation_active(key) || self.task_chat_active(key)
    }
    fn cancel_task(&mut self);
    fn cancel_task_for(&mut self, _ticket: &str) {
        self.cancel_task();
    }
    fn implementation_capacity(&self) -> bool {
        !self.is_busy()
    }
    fn max_parallel_tasks(&self) -> usize {
        3
    }
    fn active_task_count(&self) -> usize {
        0
    }
    fn set_max_parallel_tasks(&mut self, _count: usize) {}
    fn task_offer(&self) -> Option<&crate::core::workflow::InterviewBrief>;
    fn implementation_offer(&self) -> bool { false }
    fn feature_actions(&self, _conversation: Option<&str>) -> Vec<feature_approval::Action> { Vec::new() }
    fn task_documents(&self) -> &[crate::artifacts::task_docs::TaskDocument];
    fn task_archived(&self, _ticket: &str) -> bool { false }
    fn planning_work(&self) -> Vec<crate::core::planning_work::Work> { Vec::new() }
    fn archive_task(&mut self, _ticket: &str) {}
    fn implementation_state(
        &self,
        ticket: &str,
    ) -> Option<&crate::core::implementation::Implementation>;
    fn implementation_failure(&self, _ticket: &str) -> Option<&str> { None }
    fn task_attention(&mut self, _ticket: &str, _detail: &str) -> Option<crate::core::attention::View> { None }
    fn retry_task_attention(&mut self, _ticket: &str, _detail: &str) {}
    fn implementation_active(&self, ticket: &str) -> bool;
    fn implement_task(&mut self, ticket: String);
    fn auto_mode(&self) -> bool;
    fn set_auto_mode(&mut self, enabled: bool);
    fn queue_status(&self) -> &str;
    fn live_progress(&self) -> Option<&crate::harness::LiveProgress>;
    // ------- items pane -------
    fn items(&self) -> &[OpenItem];
    fn resolved_items(&self) -> &[OpenItem] {
        &[]
    }
    fn synthetic_items(&self) -> &[OpenItem];
    fn items_len(&self) -> usize;
    fn current_user(&self) -> &CurrentUser;
    /// Category→owner configuration backing the items pane's D-14-aware
    /// partition (always present; a default-empty map off-project).
    fn stakeholders(&self) -> &crate::domain::Stakeholders;
    fn next_question_id(&self) -> Option<&str>;
    fn approve_review_item(&mut self, _id: &str) {}
    // ------- spec pane -------
    fn spec_text(&self) -> &str;
    fn active_features(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }
    fn feature_approved(&self, _id: &str) -> bool {
        false
    }
    fn approve_feature(&mut self, _id: &str) {}
    // ------- services -------
    fn toasts(&mut self) -> &mut ToastQueue;
    fn on_intent(&mut self, intent: &Intent);
    fn on_header_action(&mut self, action: HeaderAction);
}
