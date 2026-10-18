pub(crate) fn current(
    store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &crate::core::lifecycle::PlanSnapshotV1,
) -> Result<(), String> {
    use crate::core::planning::{QuestionStore, ReadinessResult, assess_readiness};

    let questions = QuestionStore::new(store);
    let Some((checklist, _)) = questions
        .load_readiness(&snapshot.feature_uid)
        .map_err(|error| format!("persisted readiness could not be loaded: {error}"))?
    else {
        return Err("no persisted readiness assessment exists for the current plan".into());
    };
    if checklist.spec_revision.as_deref() != Some(snapshot.spec_revision.as_str()) {
        return Err(
            "persisted readiness is missing or belongs to a different specification revision"
                .into(),
        );
    }
    if !checklist.ready {
        return Err("the current specification is not marked ready".into());
    }
    let all_questions = questions
        .list()
        .map_err(|error| format!("clarification records could not be loaded: {error}"))?;
    let feature_questions = all_questions
        .iter()
        .filter(|question| question.feature_uid == snapshot.feature_uid)
        .collect::<Vec<_>>();
    if feature_questions
        .iter()
        .any(|question| question.plan_revision > checklist.plan_revision)
    {
        return Err("persisted readiness is stale relative to newer clarification records".into());
    }
    let assessed_questions = feature_questions
        .into_iter()
        .filter(|question| question.plan_revision == checklist.plan_revision)
        .cloned()
        .collect::<Vec<_>>();
    let requirements = snapshot
        .requirements
        .iter()
        .map(|requirement| requirement.stable_id.clone())
        .collect::<Vec<_>>();
    if !matches!(
        assess_readiness(&assessed_questions, &checklist, &requirements),
        ReadinessResult::Ready
    ) {
        return Err(
            "persisted readiness evidence no longer passes the current specification".into(),
        );
    }
    Ok(())
}
