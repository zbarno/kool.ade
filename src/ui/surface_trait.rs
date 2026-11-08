use crate::domain::chatlog::ChatMessage;

use super::{
    ApplicationCommand, FeatureSpecificationStatus, ToastQueue, feature_approval, planning_board,
    task_detail,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepositoryGitOptions {
    pub id: String,
    pub label: String,
    pub source_branches: Vec<String>,
    pub destination_branches: Vec<String>,
    pub default_branch: String,
    pub unavailable_reason: Option<String>,
}

/// Everything the chrome/panes need: read-mostly accessors plus two command
/// sinks (composer intents, header actions).
pub trait Surface {
    // ------- header -------
    fn session_title(&self) -> &str;
    fn is_git_repo(&self) -> bool;
    fn git_branch(&self) -> &str;
    fn git_checkout_label(&self) -> String {
        "Primary code repository".into()
    }
    fn repository_branches(&self) -> Vec<String> {
        Vec::new()
    }
    fn repository_destination_branches(&self) -> Vec<String> {
        self.repository_branches()
    }
    fn default_repository_branch(&self) -> &str {
        ""
    }
    fn repository_git_options(&self) -> Vec<RepositoryGitOptions> {
        if !self.is_git_repo() {
            return Vec::new();
        }
        vec![RepositoryGitOptions {
            id: "root".into(),
            label: "Primary code repository".into(),
            source_branches: self.repository_branches(),
            destination_branches: self.repository_destination_branches(),
            default_branch: self.default_repository_branch().to_owned(),
            unavailable_reason: None,
        }]
    }
    fn feature_git_intents(
        &self,
        _feature_id: &str,
    ) -> std::collections::BTreeMap<String, crate::core::git_intent::GitIntentV1> {
        std::collections::BTreeMap::new()
    }
    fn harness_settings(&self) -> crate::persistence::harness_settings::HarnessSettings {
        crate::persistence::harness_settings::load().0
    }
    fn local_user_settings_draft(&self) -> Option<crate::app::dialogs::DlgSettings> {
        None
    }
    fn save_local_user_settings(
        &mut self,
        _draft: &mut crate::app::dialogs::DlgSettings,
    ) -> Result<String, String> {
        Err("Local identity settings are unavailable in this workspace.".into())
    }
    fn project_settings_draft(&self) -> Option<crate::app::dialogs::DlgProjectSettings> {
        None
    }
    fn save_project_settings(
        &mut self,
        _draft: &mut crate::app::dialogs::DlgProjectSettings,
    ) -> Result<Option<String>, String> {
        Err("Project settings are unavailable in this workspace.".into())
    }
    fn git_head(&self) -> &str;
    fn git_dirty(&self) -> bool;
    // ------- chat pane -------
    fn chat_messages(&self) -> &[ChatMessage];
    fn specification_chat_messages(&self) -> Vec<ChatMessage> {
        self.chat_messages()
            .iter()
            .filter(|message| message.specification_context.is_some())
            .cloned()
            .collect()
    }
    fn legacy_chat_messages(&self) -> Vec<ChatMessage> {
        self.chat_messages()
            .iter()
            .filter(|message| message.specification_context.is_none())
            .cloned()
            .collect()
    }
    fn manager_activity(&self) -> &[ChatMessage] {
        &[]
    }
    fn reviewed_baseline_evidence(
        &self,
        _review_id: &str,
    ) -> Vec<crate::domain::chatlog::VerifiedRepositoryEvidence> {
        Vec::new()
    }
    fn manager_running(&self) -> bool {
        false
    }
    fn manager_progress(&self) -> Option<&crate::harness::LiveProgress> {
        None
    }
    fn manager_history_error(&self) -> Option<&str> {
        None
    }
    fn manager_last_error(&self) -> Option<&str> {
        None
    }
    fn retry_manager_check(&mut self) {}
    fn specification_chat_busy(&self) -> bool {
        self.conversation_busy()
    }
    fn specification_chat_progress(&self) -> Option<&crate::harness::LiveProgress> {
        self.live_progress()
    }
    fn task_messages(&self, _key: &str) -> &[ChatMessage] {
        &[]
    }
    fn load_runtime_qa_screenshots(
        &self,
        _ticket: &str,
        _qa_run_id: &str,
        _offset: usize,
        _limit: usize,
    ) -> Result<task_detail::RuntimeQaScreenshotGalleryView, String> {
        Err("Runtime QA screenshots are unavailable in this workspace.".into())
    }
    fn lifecycle_plan_review(
        &self,
        _feature_uid: &str,
    ) -> Result<task_detail::LifecyclePlanReviewView, String> {
        Err("Lifecycle plan review evidence is unavailable in this workspace.".into())
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
    fn planning_board(&self) -> planning_board::ViewModel {
        planning_board::ViewModel::default()
    }
    fn setup_image_preparation(&mut self, _ui: &mut egui::Ui, _prerequisite_id: &str) {}
    fn supports_provider_install(&self, _task_id: &str) -> bool {
        false
    }
    fn setup_provider_install(&mut self, _ui: &mut egui::Ui, _task_id: &str) {}
    fn bootstrap_conversation(
        &mut self,
        _task_id: &str,
    ) -> planning_board::BootstrapConversationView {
        planning_board::BootstrapConversationView::default()
    }
    fn bootstrap_conversation_draft(&mut self, _task_id: &str) -> Option<&mut String> {
        None
    }
    fn implementation_state(
        &self,
        ticket: &str,
    ) -> Option<&crate::core::implementation::Implementation>;
    fn implementation_failure(&self, _ticket: &str) -> Option<&str> {
        None
    }
    fn implementation_recovery(
        &self,
        _ticket: &str,
    ) -> Option<crate::core::implementation::RecoveryDisposition> {
        None
    }
    fn task_detail_view(&mut self, ticket: &str) -> Option<task_detail::ViewModel>;
    fn implementation_active(&self, ticket: &str) -> bool;
    fn implementation_waiting_for_capacity(&self, _ticket: &str) -> bool {
        false
    }
    fn implementation_elapsed(&self, ticket: &str) -> Option<String>;
    fn auto_plan(&self) -> bool;
    fn auto_build(&self) -> bool;
    fn auto_implement(&self) -> bool;
    fn auto_publish(&self) -> bool;
    fn require_independent_checks(&self) -> bool;
    fn queue_status(&self) -> &str;
    fn live_progress(&self) -> Option<&crate::harness::LiveProgress>;
    fn active_planning_work(&self) -> Option<&str> {
        None
    }
    // ------- items pane -------
    fn next_question_id(&self) -> Option<&str>;
    // ------- spec pane -------
    fn spec_text(&self) -> &str;
    fn has_published_product_spec(&self) -> bool {
        false
    }
    fn published_spec_pin(&self) -> Option<&crate::core::published_specs::PublishedSpecPin> {
        None
    }
    fn local_spec_draft(&self) -> Option<&crate::core::apply::LocalSpecDraftV1> {
        None
    }
    fn active_features(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }
    fn specification_chat_target(
        &self,
        scope: crate::core::specification_chat::Scope,
    ) -> Result<crate::core::specification_chat::Target, String> {
        let _ = scope;
        Err("Specification conversation context is unavailable.".into())
    }
    fn feature_approved(&self, _id: &str) -> bool {
        false
    }
    fn feature_specification_status(
        &self,
        _feature_markdown: &str,
    ) -> Result<Option<FeatureSpecificationStatus>, String> {
        Ok(None)
    }
    fn product_specification_status(
        &self,
    ) -> Result<Option<crate::core::lifecycle::ProductSpecAggregateV1>, String> {
        Ok(None)
    }
    fn requirements_for_specification_reference(
        &self,
        _feature_uid: &str,
        _reference: &str,
    ) -> Vec<crate::core::lifecycle::SpecRequirementTraceV1> {
        Vec::new()
    }
    fn specification_trace_navigation(
        &self,
    ) -> Option<crate::core::lifecycle::SpecTraceNavigationV1> {
        None
    }
    // ------- services -------
    fn toasts(&mut self) -> &mut ToastQueue;
    fn dispatch(&mut self, command: ApplicationCommand);
}
