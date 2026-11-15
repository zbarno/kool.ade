use crate::{
    app::session::Project,
    core::planning_work::{WorkKind, WorkStatus},
};

pub(super) fn next_approved(
    project: &Project,
    test_harness_override: bool,
) -> Option<(String, String)> {
    if project.active_turn.is_some()
        || project.activity.manager.is_some()
        || project.activity.plan_review.is_some()
        || project.activity.code_review_arbitration.is_some()
    {
        return None;
    }

    project.planning_work.iter().find_map(|work| {
        if work.kind != WorkKind::TaskGeneration
            || work.status != WorkStatus::Todo
            || project
                .cancelled_work
                .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
        {
            return None;
        }
        let feature_id = work.feature_id.as_deref()?;
        let feature_uid = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == feature_id)
            .and_then(|(_, markdown)| {
                crate::domain::ArtifactIdentity::from_markdown(markdown)
                    .ok()
                    .flatten()
            })?
            .uid;
        if work.feature_uid.as_deref() != Some(feature_uid.as_str())
            || (project.has_selected_code_repository()
                && !crate::app::root::planning_work::task_generation_has_approved_git_intents(
                    project,
                    feature_id,
                    Some(&feature_uid),
                ))
            || (!test_harness_override
                && !crate::app::root::harness_selection::has_selected_route_for_project_task(
                    &crate::persistence::harness_settings::load().0,
                    &project.state.harness_routes.routes,
                    crate::persistence::harness_settings::TASK_GENERATION,
                    &work.routing_overrides,
                ))
            || !crate::core::workflow::feature_approved(
                &project.state.planning_store,
                &project.state.workflow,
                feature_id,
            )
        {
            return None;
        }
        Some((work.key.clone(), feature_id.to_owned()))
    })
}

#[cfg(test)]
#[path = "task_generation/tests.rs"]
mod tests;
