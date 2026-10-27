use super::*;

mod activity;
mod conversation;
mod details;
mod plan_review;
mod project;
mod runtime_qa;
mod setup;
mod workflow;

impl Surface for KooladeApp {
    setup::setup_surface_methods!();
    project::project_surface_methods!();
    conversation::conversation_surface_methods!();
    runtime_qa::runtime_qa_surface_methods!();
    plan_review::plan_review_surface_methods!();
    activity::activity_surface_methods!();
    workflow::workflow_surface_methods!();
    fn bootstrap_conversation(
        &mut self,
        task_id: &str,
    ) -> crate::ui::planning_board::BootstrapConversationView {
        self.bootstrap_conversation.view(task_id)
    }

    fn bootstrap_conversation_draft(&mut self, task_id: &str) -> Option<&mut String> {
        self.bootstrap_conversation.draft_mut(task_id)
    }
    details::specification_status::specification_status_surface_methods!();
}
