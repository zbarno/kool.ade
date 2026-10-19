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
        Ok(output) => {
            match planner_identity {
                Some((feature_uid, plan_revision, spec_revision)) => {
                    let resolution_count = output.resolved_questions.len();
                    match crate::app::root::planner_pass::persist(
                        &project.state.planning_store,
                        &feature_uid,
                        plan_revision,
                        &spec_revision,
                        output,
                    ) {
                        Ok(assessment) => {
                            match crate::core::state::PlannerState::load_with_store(
                                &project.state.repo_root,
                                &project.state.planning_store,
                            ) {
                                Ok(state) => {
                                    project.state = state;
                                    let resolution_note = if resolution_count == 0 {
                                        String::new()
                                    } else {
                                        format!(
                                            " Recorded {resolution_count} repository-evidenced technical decision(s) in the feature specification."
                                        )
                                    };
                                    match assessment {
                                crate::core::planning::ReadinessResult::Ready =>
                                    format!("Planner saved follow-up questions and readiness evidence; the plan is ready for review.{resolution_note}"),
                                crate::core::planning::ReadinessResult::NeedsWork { blockers, unassessed, untraced_requirements, missing_acceptance_tests } =>
                                    format!("Planner saved follow-up questions and readiness evidence; plan remains unready ({} blockers, {} unassessed axes, {} untraced requirements, missing acceptance tests: {}).{resolution_note}", blockers.len(), unassessed.len(), untraced_requirements.len(), missing_acceptance_tests),
                                crate::core::planning::ReadinessResult::PlanningStalled { .. } =>
                                    format!("Planner saved evidence, but readiness is stalled by repeated no-progress results.{resolution_note}"),
                                crate::core::planning::ReadinessResult::InvalidEvidence { reasons } =>
                                    format!("Planner saved records, but readiness evidence is invalid: {}.{resolution_note}", reasons.join("; ")),
                                }
                                }
                                Err(error) => format!(
                                    "Planner records were saved, but the updated feature specification could not be reloaded: {error}"
                                ),
                            }
                        }
                        Err(error) => format!("Planner output could not be persisted: {error}"),
                    }
                }
                None => {
                    "Planner output was discarded because its feature context is no longer active."
                        .into()
                }
            }
        }
        Err(error) => format!("The clarification Planner pass did not complete: {error}"),
    };
    project.remember_chat(vec![crate::domain::chatlog::ChatMessage::new(
        crate::domain::chatlog::ChatRole::System,
        message,
        None,
    )]);
}
