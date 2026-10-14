pub(super) fn advance_clarification_planner(project: &mut crate::app::session::Project) {
    let result = project
        .activity
        .clarification_planner
        .as_ref()
        .and_then(crate::app::root::planner_pass::ClarificationPlanner::take_result);
    let Some(result) = result else { return };
    project.activity.clarification_planner = None;
    project.live_progress = Default::default();
    let planner_identity = project.activity.clarification_planner_identity.take();
    let message = match result {
        Ok(output) => match planner_identity {
            Some((feature_uid, plan_revision, spec_revision)) => {
                match crate::app::root::planner_pass::persist(
                        &project.state.planning_store,
                        &feature_uid,
                        plan_revision,
                        &spec_revision,
                        output,
                    ) {
                        Ok(crate::core::planning::ReadinessResult::Ready) =>
                            "Planner saved follow-up questions and readiness evidence; the plan is ready for review.".to_owned(),
                        Ok(crate::core::planning::ReadinessResult::NeedsWork { blockers, unassessed, untraced_requirements, missing_acceptance_tests }) => format!(
                            "Planner saved follow-up questions and readiness evidence; plan remains unready ({} blockers, {} unassessed axes, {} untraced requirements, missing acceptance tests: {}).",
                            blockers.len(), unassessed.len(), untraced_requirements.len(), missing_acceptance_tests
                        ),
                        Ok(crate::core::planning::ReadinessResult::PlanningStalled { .. }) =>
                            "Planner saved evidence, but readiness is stalled by repeated no-progress results.".into(),
                        Ok(crate::core::planning::ReadinessResult::InvalidEvidence { reasons }) =>
                            format!("Planner saved records, but readiness evidence is invalid: {}", reasons.join("; ")),
                        Err(error) => format!("Planner output could not be persisted: {error}"),
                    }
            }
            None => "Planner output was discarded because its feature context is no longer active."
                .into(),
        },
        Err(error) => format!("The clarification Planner pass did not complete: {error}"),
    };
    project.remember_chat(vec![crate::domain::chatlog::ChatMessage::new(
        crate::domain::chatlog::ChatRole::System,
        message,
        None,
    )]);
}
