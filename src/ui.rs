//! Presentation layer (egui). Views draw exclusively through the
//! [`Surface`] trait, so the app state machine (`crate::app`) stays
//! decoupled from pixels.

pub mod chat_pane;
pub(crate) mod decision_choice;
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
pub mod task_detail;
pub mod theme;
pub mod toast;

pub use chat_pane::Intent;
pub use layout::HeaderAction;
pub use toast::ToastQueue;

/// Typed actions emitted by views and handled by the application state
/// machine. UI code reads state through `Surface` and reports intent here.
pub enum ApplicationCommand {
    PrepareTaskChat { key: String },
    SendTaskReply { key: String },
    SendImplementationDecision { key: String },
    CancelTaskReply { key: String },
    RetryTaskChatSave,
    DrainTaskChatSaves,
    CancelTask,
    SetMaxParallelTasks { count: usize },
    SetAutoPlan { enabled: bool },
    SetAutoBuild { enabled: bool },
    SetAutoPublish { enabled: bool },
    SetRequireIndependentChecks { enabled: bool },
    ArchiveTask { ticket: String },
    ApproveReviewItem { id: String },
    ApproveFeature { id: String },
    UserIntent(Intent),
    HeaderAction(HeaderAction),
    TaskDetail(task_detail::Command),
}

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
    fn task_chat_context(&self, _key: &str) -> Option<String> {
        None
    }
    fn task_draft(&mut self, _key: &str) -> Option<&mut String> {
        None
    }
    fn task_chat_active(&self, _key: &str) -> bool {
        false
    }
    fn task_reply_progress(&self, _key: &str) -> Option<&crate::harness::LiveProgress> {
        None
    }
    fn task_reply_busy(&self) -> bool {
        self.conversation_busy()
    }
    fn task_chat_error(&self) -> Option<&str> {
        None
    }
    /// Best-effort per-frame flush of unsaved task-conversation replies; a
    /// no-op unless a previous save failed, so a transient store hiccup does
    /// not strand the last remembered reply.
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
    fn max_parallel_tasks(&self) -> usize {
        3
    }
    fn active_task_count(&self) -> usize {
        0
    }
    fn task_offer(&self) -> Option<&crate::core::workflow::InterviewBrief>;
    fn implementation_offer(&self) -> bool {
        false
    }
    fn feature_actions(&self, _conversation: Option<&str>) -> Vec<feature_approval::Action> {
        Vec::new()
    }
    fn task_documents(&self) -> &[crate::artifacts::task_docs::TaskDocument];
    fn task_archived(&self, _ticket: &str) -> bool {
        false
    }
    fn planning_work(&self) -> Vec<crate::core::planning_work::Work> {
        Vec::new()
    }
    fn implementation_state(
        &self,
        ticket: &str,
    ) -> Option<&crate::core::implementation::Implementation>;
    fn implementation_failure(&self, _ticket: &str) -> Option<&str> {
        None
    }
    fn task_detail_view(&mut self, ticket: &str) -> Option<task_detail::ViewModel>;
    fn implementation_active(&self, ticket: &str) -> bool;
    fn auto_plan(&self) -> bool;
    fn auto_build(&self) -> bool;
    fn auto_publish(&self) -> bool;
    fn require_independent_checks(&self) -> bool;
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
    // ------- spec pane -------
    fn spec_text(&self) -> &str;
    fn active_features(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }
    fn feature_approved(&self, _id: &str) -> bool {
        false
    }
    // ------- services -------
    fn toasts(&mut self) -> &mut ToastQueue;
    fn dispatch(&mut self, command: ApplicationCommand);
}
