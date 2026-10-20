use super::super::{KooladeApp, Screen};

impl KooladeApp {
    pub(crate) fn approve_feature_from_board(&mut self, id: &str) {
        if matches!(&self.screen, Screen::Connected(project) if project.activity.plan_review_feature.as_deref() == Some(id))
        {
            return;
        }
        if self
            .available_feature_actions(None)
            .iter()
            .any(|action| action.id == id && action.retry_review)
        {
            self.retry_plan_review(id);
            return;
        }
        // A second board action after independent review should update the
        // generation card, not write a fresh confirmation receipt. Replacing
        // that receipt would make the already recorded review stale.
        let already_approved = matches!(&self.screen, Screen::Connected(project) if project
        .state
        .active_features
        .iter()
        .find(|(feature_id, _)| feature_id == id)
        .is_some_and(|(_, body)| {
            let contract = crate::core::workflow::feature_contract(body);
            crate::core::workflow::feature_approved(
                &project.state.planning_store,
                &project.state.workflow,
                id,
            ) && crate::persistence::feature_approvals::contains(
                &project.state.planning_store.git_root(),
                id,
                &contract,
            )
        }));
        if !already_approved {
            self.approve_feature_only(id);
        }
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let Some((_, body)) = project
            .state
            .active_features
            .iter()
            .find(|(feature_id, _)| feature_id == id)
        else {
            return;
        };
        let contract = crate::core::workflow::feature_contract(body);
        if !crate::persistence::feature_approvals::contains(
            &project.state.planning_store.git_root(),
            id,
            &contract,
        ) {
            return;
        }
        let approved = crate::core::workflow::feature_approved(
            &project.state.planning_store,
            &project.state.workflow,
            id,
        );
        let reviewing = project.activity.plan_review_feature.as_deref() == Some(id);
        let status = if approved {
            crate::core::planning_work::WorkStatus::Todo
        } else {
            crate::core::planning_work::WorkStatus::NeedsAttention
        };
        let detail = if approved {
            "Approved: the independent PlanReviewer approved this exact plan snapshot. Task generation is ready when requested.".to_owned()
        } else if reviewing {
            "Blocked: the independent PlanReviewer is reviewing the exact confirmed plan snapshot."
                .to_owned()
        } else {
            "Blocked: independent PlanReviewer approval is required for the exact confirmed plan snapshot before task generation.".to_owned()
        };
        let Ok(Some(identity)) = crate::domain::ArtifactIdentity::from_markdown(body) else {
            return;
        };
        if let Some(work) = project.planning_work.iter_mut().find(|work| {
            work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                && work.feature_uid.as_deref() == Some(identity.uid.as_str())
        }) {
            work.status = status;
            work.detail = detail;
            if let Err(error) = project.save_planning_work() {
                self.toasts.danger(format!("Plan confirmation was recorded, but the review blocker could not be saved: {error}"));
            }
            return;
        }
        let title = body
            .lines()
            .next()
            .unwrap_or(id)
            .trim_start_matches('#')
            .trim();
        let mut work = crate::core::planning_work::Work::new(
            String::new(),
            format!("Generate tasks: {title}"),
            format!(
                "Generate implementation tasks for confirmed feature {id} after independent plan review."
            ),
            detail,
        );
        work.kind = crate::core::planning_work::WorkKind::TaskGeneration;
        work.status = status;
        work.feature_id = Some(id.to_owned());
        work.feature_uid = Some(identity.uid);
        work.key = format!("task-generation:{}", work.uid);
        project.planning_work.push(work);
        if let Err(error) = project.save_planning_work() {
            project.planning_work.pop();
            self.toasts.danger(format!(
                "Feature was approved, but the task generation card could not be saved: {error}"
            ));
            return;
        }
        self.toasts.warning(format!(
            "{id} is confirmed. Task generation is blocked until the independent PlanReviewer approves this exact snapshot."
        ));
    }
}
