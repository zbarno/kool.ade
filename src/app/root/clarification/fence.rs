use crate::{
    app::session::Project,
    core::planning::{QuestionStore, QuestionV1, ReadinessChecklistV1},
};

pub(super) fn current_question_planning_revision(
    project: &Project,
    store: &QuestionStore<'_>,
    question: &QuestionV1,
) -> Result<String, String> {
    let planning_revision = project
        .state
        .planning_store
        .revision()
        .map_err(|error| error.to_string())?;
    let persisted_feature =
        crate::artifacts::product_docs::active_features(&project.state.planning_store)
            .into_iter()
            .find(|(_, markdown)| {
                crate::domain::ArtifactIdentity::from_markdown(markdown)
                    .ok()
                    .flatten()
                    .is_some_and(|identity| identity.uid == question.feature_uid)
            })
            .ok_or_else(|| "persisted feature is no longer active".to_owned())?;
    let (feature_id, markdown) = persisted_feature;
    let persisted_spec_revision = crate::app::root::planner_pass::spec_revision_for_markdown(
        &project.state.planning_store,
        &question.feature_uid,
        &feature_id,
        &markdown,
    )?;
    let app_spec_revision =
        crate::app::root::planner_pass::spec_revision(project, &question.feature_uid)?;
    let (checklist, _) = store
        .load_readiness(&question.feature_uid)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no readiness evidence exists for the current plan".to_owned())?;
    ensure_question_snapshot_is_current(
        question.plan_revision,
        &checklist,
        &persisted_spec_revision,
        &app_spec_revision,
    )?;
    Ok(planning_revision)
}

fn ensure_question_snapshot_is_current(
    question_plan_revision: u64,
    checklist: &ReadinessChecklistV1,
    persisted_spec_revision: &str,
    app_spec_revision: &str,
) -> Result<(), String> {
    if checklist.plan_revision != question_plan_revision
        || checklist.spec_revision.as_deref() != Some(persisted_spec_revision)
        || app_spec_revision != persisted_spec_revision
    {
        return Err("question, readiness, and active feature snapshots do not match".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "fence_tests.rs"]
mod tests;
