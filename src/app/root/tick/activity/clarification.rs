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
                let resolved_questions = output.resolved_questions.clone();
                match crate::app::root::planner_pass::persist(
                    &project.state.planning_store,
                    &feature_uid,
                    plan_revision,
                    &spec_revision,
                    output,
                ) {
                    Ok(assessment) => {
                        queue_evidence_resolutions(project, &resolved_questions);
                        match assessment {
                                crate::core::planning::ReadinessResult::Ready =>
                            "Planner saved follow-up questions and readiness evidence; the plan is ready for review.".to_owned()
                                ,
                                crate::core::planning::ReadinessResult::NeedsWork { blockers, unassessed, untraced_requirements, missing_acceptance_tests } =>
                            format!(
                            "Planner saved follow-up questions and readiness evidence; plan remains unready ({} blockers, {} unassessed axes, {} untraced requirements, missing acceptance tests: {}).",
                            blockers.len(), unassessed.len(), untraced_requirements.len(), missing_acceptance_tests
                            ),
                                crate::core::planning::ReadinessResult::PlanningStalled { .. } =>
                            "Planner saved evidence, but readiness is stalled by repeated no-progress results.".into(),
                                crate::core::planning::ReadinessResult::InvalidEvidence { reasons } =>
                            format!("Planner saved records, but readiness evidence is invalid: {}", reasons.join("; ")),
                            }
                    }
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

fn queue_evidence_resolutions(
    project: &mut crate::app::session::Project,
    resolutions: &[crate::harness::prompt_catalog::TechnicalResolutionV1],
) {
    for event in evidence_resolution_events(resolutions) {
        project.activity.pending.push(event);
    }
}

fn evidence_resolution_events(
    resolutions: &[crate::harness::prompt_catalog::TechnicalResolutionV1],
) -> Vec<String> {
    if resolutions.is_empty() {
        return Vec::new();
    }
    resolutions
        .chunks(8)
        .map(|chunk| {
            let details = chunk
                .iter()
                .map(|resolution| {
                    let evidence = resolution
                        .evidence_refs
                        .iter()
                        .take(12)
                        .map(|reference| crate::core::context_build::clip(reference, 128))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "- {}: {} ({}; evidence: {})",
                        resolution.question_uid,
                        crate::core::context_build::clip(&resolution.decision, 600),
                        crate::core::context_build::clip(&resolution.rationale, 900),
                        evidence
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "Repository evidence resolved technical clarification questions. Update the living specification's technical decisions/assumptions with the supported choices below, cite the evidence, and preserve user-owned decisions and approved scope. Do not approve or implement work from this event.\n{details}"
            )
        })
        .collect()
}

#[cfg(test)]
#[path = "clarification/tests.rs"]
mod tests;
