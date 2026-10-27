use crate::domain::chatlog::ChatMessage;

use super::{
    ApplicationCommand, FeatureSpecificationStatus, PlanningRepositoryViewModel, ToastQueue,
    feature_approval, planning_board, task_detail,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryChoice {
    pub id: String,
    pub label: String,
}

/// Everything the chrome/panes need: read-mostly accessors plus two command
/// sinks (composer intents, header actions).
pub trait Surface {
    // ------- header -------
    fn session_title(&self) -> &str;
    fn is_git_repo(&self) -> bool;
    fn registered_repositories(&self) -> Vec<RepositoryChoice> {
        Vec::new()
    }
    fn can_register_code_repository(&self) -> bool {
        false
    }
    fn git_branch(&self) -> &str;
    fn repository_branches(&self) -> Vec<String> {
        Vec::new()
    }
    fn repository_destination_branches(&self) -> Vec<String> {
        self.repository_branches()
    }
    fn default_repository_branch(&self) -> &str {
        self.git_branch()
    }
    fn harness_settings(&self) -> crate::persistence::harness_settings::HarnessSettings {
        crate::persistence::harness_settings::load().0
    }
    fn people_settings_draft(&self) -> Option<crate::app::dialogs::DlgSettings> {
        None
    }
    fn save_people_settings(
        &mut self,
        _draft: &mut crate::app::dialogs::DlgSettings,
    ) -> Result<String, String> {
        Err("People settings are unavailable in this workspace.".into())
    }
    fn project_settings_draft(&self) -> Option<crate::app::dialogs::DlgProjectSettings> {
        None
    }
    fn planning_repository(&self) -> Option<PlanningRepositoryViewModel> {
        None
    }
    fn save_planning_remote(&mut self, _remote: &str) -> Result<(), String> {
        Err("Planning repository settings are unavailable in this workspace.".into())
    }
    fn migration_preview(
        &self,
    ) -> Result<crate::core::managed_workspace::MigrationPreview, String> {
        Err("Legacy migration preview is unavailable in this workspace.".into())
    }
    fn confirm_planning_migration(
        &mut self,
        _preview: &crate::core::managed_workspace::MigrationPreview,
    ) -> Result<(), String> {
        Err("Legacy migration is unavailable in this workspace.".into())
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
    fn stale_task_claim(&self) -> Option<(String, crate::core::task_claim::ClaimRecord)> {
        None
    }
    fn live_progress(&self) -> Option<&crate::harness::LiveProgress>;
    fn active_planning_work(&self) -> Option<&str> {
        None
    }
    // ------- items pane -------
    fn next_question_id(&self) -> Option<&str>;
    // ------- spec pane -------
    fn spec_text(&self) -> &str;
    fn active_features(&self) -> Vec<(&str, &str)> {
        Vec::new()
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
