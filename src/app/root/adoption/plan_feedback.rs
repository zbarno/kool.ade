use crate::app::session::Project;

pub(super) struct RefreshTarget {
    feature_id: String,
    feature_uid: String,
    prior_spec_revision: String,
    next_plan_revision: u64,
}

pub(super) fn capture(project: &Project, work_key: Option<&str>) -> Option<RefreshTarget> {
    let work = project.planning_work.iter().find(|work| {
        Some(work.key.as_str()) == work_key
            && (work.key.starts_with("plan-confirmation-changes:")
                || work.key.starts_with("task:manager-plan-repair:"))
            && work.kind == crate::core::planning_work::WorkKind::PlanReviewDisposition
    })?;
    let feature_id = work.feature_id.as_ref()?;
    let markdown = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == feature_id)
        .map(|(_, markdown)| markdown)?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .ok()
        .flatten()?;
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        feature_id,
        &identity,
        markdown,
        &crate::core::workflow::feature_contract(markdown),
    )
    .ok()?;
    let prior_plan_revision =
        crate::core::planning::QuestionStore::new(&project.state.planning_store)
            .load_readiness(&identity.uid)
            .ok()
            .flatten()
            .map_or(0, |(readiness, _)| readiness.plan_revision);
    Some(RefreshTarget {
        feature_id: feature_id.clone(),
        feature_uid: identity.uid,
        prior_spec_revision: snapshot.spec_revision,
        next_plan_revision: prior_plan_revision.checked_add(1)?,
    })
}

pub(super) fn schedule(
    project: &mut Project,
    target: Option<&RefreshTarget>,
    updates: &[(String, String)],
) {
    let Some(target) = target else { return };
    if !updates
        .iter()
        .any(|(document_id, _)| document_id == &format!("feature:{}", target.feature_id))
    {
        return;
    }
    let Some(markdown) = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == &target.feature_id)
        .map(|(_, markdown)| markdown)
    else {
        return;
    };
    let Ok(Some(identity)) = crate::domain::ArtifactIdentity::from_markdown(markdown) else {
        return;
    };
    if identity.uid != target.feature_uid {
        return;
    }
    let Ok(snapshot) = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        &target.feature_id,
        &identity,
        markdown,
        &crate::core::workflow::feature_contract(markdown),
    ) else {
        return;
    };
    if snapshot.spec_revision != target.prior_spec_revision {
        project.activity.pending_clarification_planner =
            Some((target.feature_uid.clone(), target.next_plan_revision));
    }
}
