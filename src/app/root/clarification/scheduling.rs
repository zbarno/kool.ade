use crate::app::session::Project;

/// Pick the next clarification planning revision when an applied main-chat
/// turn changes the active feature snapshot.
pub(super) fn for_changed_active_feature(
    project: &Project,
) -> Result<Option<(String, u64)>, String> {
    let Some((feature_id, markdown)) = project.state.active_feature.as_ref() else {
        return Ok(None);
    };
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "updated feature has no stable identity".to_owned())?;
    let current_spec_revision = super::super::planner_pass::spec_revision_for_markdown(
        &identity.uid,
        feature_id,
        markdown,
    )?;
    if project
        .activity
        .clarification_planner_identity
        .as_ref()
        .is_some_and(|(uid, _, revision)| {
            uid == &identity.uid && revision == &current_spec_revision
        })
    {
        return Ok(None);
    }
    let questions = crate::core::planning::QuestionStore::new(&project.state.planning_store);
    let current_questions = questions.list().map_err(|error| error.to_string())?;
    let existing_readiness = questions
        .load_readiness(&identity.uid)
        .map_err(|error| error.to_string())?;
    if existing_readiness.as_ref().is_some_and(|(readiness, _)| {
        readiness.spec_revision.as_deref() == Some(current_spec_revision.as_str())
    }) {
        return Ok(None);
    }
    let latest_plan_revision = current_questions
        .iter()
        .filter(|question| question.feature_uid == identity.uid)
        .map(|question| question.plan_revision)
        .chain(
            existing_readiness
                .iter()
                .map(|(readiness, _)| readiness.plan_revision),
        )
        .max();
    let next_revision = match latest_plan_revision {
        Some(revision) => revision
            .checked_add(1)
            .ok_or_else(|| "clarification planning revision is exhausted".to_owned())?,
        None => 0,
    };
    Ok(Some((identity.uid, next_revision)))
}
