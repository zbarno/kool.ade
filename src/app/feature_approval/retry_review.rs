use super::*;

impl KooladeApp {
    pub(super) fn retry_plan_review(&mut self, id: &str) {
        let screen = &mut self.screen;
        #[cfg(not(test))]
        let harness_slot = &mut self.task_harness;
        let Screen::Connected(project) = screen else {
            return;
        };
        if project.active_turn.is_some() || project.activity.plan_review.is_some() {
            return;
        }
        let Some((_, markdown)) = project
            .state
            .active_features
            .iter()
            .find(|(feature_id, _)| feature_id == id)
        else {
            return;
        };
        let Some(identity) = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .ok()
            .flatten()
        else {
            return;
        };
        let contract = workflow::feature_contract(markdown);
        let Ok(snapshot) = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
            &project.state.planning_store,
            id,
            &identity,
            markdown,
            &contract,
        ) else {
            return;
        };
        let confirmed = crate::core::lifecycle::plan_snapshot::has_current_user_confirmation(
            &project.state.planning_store,
            &snapshot,
        )
        .unwrap_or(false);
        let retry_available = crate::core::lifecycle::plan_snapshot::review_retry_available(
            &project.state.planning_store,
            &snapshot,
        )
        .unwrap_or(false);
        let review_not_started = crate::core::lifecycle::plan_snapshot::plan_review_receipt(
            &project.state.planning_store,
            &snapshot,
        )
        .is_ok_and(|receipt| receipt.is_none());
        let authorized = confirmed && (retry_available || review_not_started);
        if !authorized {
            return;
        }
        #[cfg(test)]
        let review_start: Result<_, String> =
            Err("test-only reviewer outcomes must be supplied explicitly".into());
        #[cfg(not(test))]
        let review_start = {
            let harness = crate::app::root::configured_harness_for_project(
                harness_slot,
                Some(crate::persistence::harness_settings::PLAN_REVIEW),
                &project.state.harness_routes.routes,
            );
            crate::app::root::plan_review::PlanReview::start(project, id.to_owned(), harness)
        };
        match review_start {
            Ok(review) => {
                project.activity.plan_review = Some(review);
                project.activity.plan_review_feature = Some(id.to_owned());
                project.activity.plan_review_revision =
                    Some(project.state.baseline_planning_revision.clone());
                project.live_progress = crate::harness::LiveProgress {
                    activity: Some(format!("Independent PlanReviewer is retrying {id}…")),
                    ..Default::default()
                };
                if let Some(work) = project.planning_work.iter_mut().find(|work| {
                    work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                        && work.feature_uid.as_deref() == Some(identity.uid.as_str())
                }) {
                    work.status = crate::core::planning_work::WorkStatus::InProgress;
                    work.detail = "ReviewUnable was recorded. A bounded independent review retry is running; task generation remains blocked.".into();
                    if let Err(error) = project.save_planning_work() {
                        project.activity.pending.push(format!(
                            "Plan review retry started, but its board status could not be saved: {error}"
                        ));
                    }
                }
            }
            Err(error) => {
                project.activity.pending.push(format!(
                    "Independent plan review retry for {id} could not start: {error}"
                ));
            }
        }
    }
}
