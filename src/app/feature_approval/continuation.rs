use super::*;

impl KooladeApp {
    /// Only successful completion of the Main Chat review can consume this continuation.
    pub(in crate::app::root) fn continue_feature_generation(&mut self, applied: bool) {
        let Some((repo, id, contract, work_key)) = self.pending_feature_generation.take() else {
            return;
        };
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.state.repo_root != repo {
            return;
        }
        let current = project
            .state
            .active_feature
            .as_ref()
            .is_some_and(|(active, text)| {
                active == &id && workflow::feature_contract(text) == contract
            });
        if applied
            && current
            && project
                .state
                .workflow
                .ready(project.state.planning_contract())
            && !has_current_task_batch(project)
            && workflow::feature_approved(
                &project.state.planning_store,
                &project.state.workflow,
                &id,
            )
        {
            self.start_turn_for_work(
                &format!("Generate task stories for approved feature {id}."),
                TurnPurpose::GenerateTasks,
                None,
                (!work_key.is_empty()).then_some(work_key),
            );
            return;
        }
        let reason = if !current {
            "The feature contract or planning focus changed; review the current feature before continuing."
        } else {
            "The planning review did not produce a current, ready task plan. Resolve the reported issue, then use Prepare tasks to retry; the existing feature approval is retained."
        };
        project.remember_chat(vec![ChatMessage::new(
            ChatRole::System,
            format!("Task preparation for {id} paused. {reason}"),
            None,
        )]);
        if !work_key.is_empty()
            && let Some(work) = project
                .planning_work
                .iter_mut()
                .find(|work| work.key == work_key)
        {
            work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
            work.detail = format!("Task generation paused. {reason}");
            if let Err(error) = project.save_planning_work() {
                self.toasts
                    .danger(format!("Cannot save the task generation status: {error}"));
            }
        }
    }
}
