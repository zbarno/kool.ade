use crate::app::session::Project;

pub(super) fn record_reviewed_plan(project: &Project, feature_id: &str) -> Result<(), String> {
    let markdown = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == feature_id)
        .map(|(_, markdown)| markdown)
        .ok_or_else(|| "approved feature specification is not loaded".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "approved feature has no stable identity".to_owned())?;
    let contract = crate::core::workflow::feature_contract(markdown);
    let store = &project.state.planning_store;
    let plan = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        store, feature_id, &identity, markdown, &contract,
    )?;
    if !crate::core::lifecycle::plan_snapshot::is_persisted(store, &plan)
        .map_err(|error| error.to_string())?
    {
        return Err("the reviewed plan snapshot is not persisted".into());
    }
    crate::app::root::feature_approval::readiness::current(store, &plan)?;
    let question_store = crate::core::planning::QuestionStore::new(store);
    let (readiness, _) = question_store
        .load_readiness(&plan.feature_uid)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "readiness evidence is missing".to_owned())?;
    if readiness.spec_revision.as_deref() != Some(plan.spec_revision.as_str()) {
        return Err("readiness evidence is bound to a stale specification".into());
    }
    let mut readiness_refs = readiness.evidence_refs;
    readiness_refs.extend(
        readiness
            .acceptance_tests
            .into_iter()
            .map(|test| test.test_ref),
    );
    readiness_refs.sort();
    readiness_refs.dedup();
    let user_receipt =
        crate::core::lifecycle::plan_snapshot::user_confirmation_receipt(store, &plan)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "the persisted user confirmation receipt is missing".to_owned())?;
    let review_receipt = crate::core::lifecycle::plan_snapshot::plan_review_receipt(store, &plan)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the persisted independent PlanReviewer receipt is missing".to_owned())?;
    crate::core::lifecycle::LifecycleStore::new(store)
        .reconcile_reviewed_plan(
            &plan,
            markdown,
            readiness_refs,
            &user_receipt,
            &review_receipt,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}
