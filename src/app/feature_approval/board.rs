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
                &project.state.planning_store.local_root(),
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
            &project.state.planning_store.local_root(),
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
            "Approved: the independent PlanReviewer approved this exact plan snapshot. Task generation starts automatically once required repository Git intents are approved.".to_owned()
        } else if reviewing {
            "Blocked: the independent PlanReviewer is reviewing the exact confirmed plan snapshot."
                .to_owned()
        } else {
            "Blocked: independent PlanReviewer approval is required for the exact confirmed plan snapshot before task generation.".to_owned()
        };
        if let Err(error) = reconcile_task_generation_work(project, id, status, detail) {
            self.toasts.danger(format!(
                "Plan confirmation was recorded, but its task-generation card could not be saved: {error}"
            ));
            return;
        }
        if !approved {
            self.toasts.warning(format!(
                "{id} is confirmed. Task generation is blocked until the independent PlanReviewer approves this exact snapshot."
            ));
        }
    }
}

/// Keep a durable task-generation action in sync for both board confirmation
/// and independently completed plan reviews. Review may finish through a path
/// that never visited the board's confirmation handler.
pub(crate) fn reconcile_task_generation_work(
    project: &mut crate::app::session::Project,
    feature_id: &str,
    status: crate::core::planning_work::WorkStatus,
    detail: String,
) -> Result<(), String> {
    use crate::core::planning_work::{Work, WorkKind};
    let (_, body) = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == feature_id)
        .ok_or_else(|| format!("Feature {feature_id} is not loaded"))?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(body)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Feature {feature_id} has no immutable identity"))?;
    let title = body
        .lines()
        .next()
        .unwrap_or(feature_id)
        .trim_start_matches('#')
        .trim()
        .to_owned();
    let previous = project.planning_work.clone();
    if let Some(work) = project.planning_work.iter_mut().find(|work| {
        work.kind == WorkKind::TaskGeneration
            && work.feature_uid.as_deref() == Some(identity.uid.as_str())
    }) {
        // An already finished generation card must never be resurrected.
        if work.status == crate::core::planning_work::WorkStatus::Done {
            return Ok(());
        }
        work.status = status;
        work.detail = detail;
    } else {
        let mut work = Work::new(
            String::new(),
            format!("Generate tasks: {title}"),
            format!(
                "Generate implementation tasks for confirmed feature {feature_id} after independent plan review."
            ),
            detail,
        );
        work.kind = WorkKind::TaskGeneration;
        work.status = status;
        work.feature_id = Some(feature_id.to_owned());
        work.feature_uid = Some(identity.uid.clone());
        work.task_intent_uid =
            Work::task_intent_uid_for_feature(&project.planning_work, &identity.uid);
        work.key = format!("task-generation:{}", work.uid);
        project.planning_work.push(work);
    }
    if let Err(error) = project.save_planning_work() {
        project.planning_work = previous;
        return Err(error.to_string());
    }
    Ok(())
}
