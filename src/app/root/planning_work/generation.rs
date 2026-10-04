use super::super::{KooladeApp, Screen};

impl KooladeApp {
    pub(crate) fn start_task_generation(&mut self, work_key: &str, feature_id: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let Some(index) = project.planning_work.iter().position(|work| {
            work.key == work_key
                && work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                && work.feature_id.as_deref() == Some(feature_id)
        }) else {
            return;
        };
        if project.active_turn.is_some() || !project.active_implementations.is_empty() {
            self.toasts.warning(
                "Wait for current planning or implementation work before generating tasks.",
            );
            return;
        }
        let Some((_, specification)) = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == feature_id)
            .cloned()
        else {
            project.planning_work[index].status =
                crate::core::planning_work::WorkStatus::NeedsAttention;
            project.planning_work[index].detail = "The approved specification is no longer available. Reopen planning and review the feature.".into();
            if let Err(error) = project.save_planning_work() {
                self.toasts
                    .danger(format!("Could not save task generation status: {error}"));
            }
            return;
        };
        if !crate::core::workflow::feature_approved(
            &project.state.repo_root,
            &project.state.workflow,
            feature_id,
        ) {
            self.toasts
                .warning("This specification needs approval before tasks can be generated.");
            return;
        }
        project.state.active_feature = Some((feature_id.to_owned(), specification.clone()));
        if super::super::has_current_task_batch(project) {
            project.planning_work[index].status = crate::core::planning_work::WorkStatus::Done;
            project.planning_work[index].detail =
                "Implementation tasks already exist for this approved specification.".into();
            if let Err(error) = project.save_planning_work() {
                self.toasts
                    .danger(format!("Could not save task generation status: {error}"));
            }
            return;
        }
        project.planning_work[index].status = crate::core::planning_work::WorkStatus::InProgress;
        project.planning_work[index].detail =
            "Checking that the approved specification is ready for task generation.".into();
        let repo = project.state.repo_root.clone();
        let contract = crate::core::workflow::feature_contract(&specification);
        if let Err(error) = project.save_planning_work() {
            project.planning_work[index].status =
                crate::core::planning_work::WorkStatus::NeedsAttention;
            project.planning_work[index].detail =
                format!("Could not save task generation progress: {error}");
            project.activity.pending_planning_work = true;
            self.toasts
                .danger(project.planning_work[index].detail.clone());
            return;
        }
        self.pending_feature_generation =
            Some((repo, feature_id.to_owned(), contract, work_key.to_owned()));
        let prompt = format!(
            "Prepare the task-generation review for approved feature {feature_id}. Its current specification is approved; do not ask for approval again or change its contract. Read the current feature and settled decisions, and return a complete interview brief naming {feature_id}, ready_for_tasks=true when no blocking questions remain. Do not generate stories in this review turn. The application will generate them after a successful current review. If blocked, record the concrete blocking item."
        );
        self.start_turn_for_work(
            &prompt,
            crate::core::workflow::TurnPurpose::ReviewForGeneration,
            None,
            Some(work_key.to_owned()),
        );
    }
}
