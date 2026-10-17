pub(super) fn required_requirement_ids_for_snapshot(
    store: &crate::artifacts::planning_store::PlanningStore,
    feature_uid: &str,
    expected_spec_revision: &str,
) -> Result<Vec<String>, String> {
    let (feature_id, markdown) = crate::artifacts::product_docs::active_features(store)
        .into_iter()
        .find(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
                .is_some_and(|identity| identity.uid == feature_uid)
        })
        .ok_or_else(|| "the planned feature is no longer active".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the planned feature has no stable identity".to_owned())?;
    let contract = crate::core::workflow::feature_contract(&markdown);
    let snapshot = crate::core::lifecycle::PlanSnapshotV1::from_feature(
        &feature_id,
        &identity,
        &markdown,
        &contract,
    )?;
    if snapshot.spec_revision != expected_spec_revision {
        return Err("planner output belongs to a stale feature snapshot".into());
    }
    Ok(snapshot
        .requirements
        .into_iter()
        .map(|requirement| requirement.stable_id)
        .collect())
}

pub(super) fn supersede_older_questions(
    questions: &crate::core::planning::QuestionStore<'_>,
    current: &[crate::core::planning::QuestionV1],
    feature_uid: &str,
    plan_revision: u64,
) -> Result<Vec<(crate::core::planning::QuestionV1, u64)>, String> {
    use crate::core::planning::{QuestionStatus, supersede_question};
    let mut superseded = Vec::new();
    for question in current.iter().filter(|question| {
        question.feature_uid == feature_uid && question.plan_revision != plan_revision
    }) {
        if question.plan_revision > plan_revision {
            return Err("planner output is older than persisted clarification state".into());
        }
        if matches!(
            question.status,
            QuestionStatus::Cancelled | QuestionStatus::Superseded
        ) {
            continue;
        }
        let (mut persisted, record_revision) = questions
            .load(&question.uid)
            .map_err(|error| error.to_string())?;
        if persisted != *question {
            return Err("clarification changed while the plan was being persisted".into());
        }
        let question_revision = persisted.revision;
        supersede_question(&mut persisted, question_revision, plan_revision)?;
        superseded.push((persisted, record_revision));
    }
    Ok(superseded)
}

pub(super) fn parse_axis(value: &str) -> Option<crate::core::planning::ReadinessAxis> {
    use crate::core::planning::ReadinessAxis as Axis;
    Some(
        match value
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-', '_'], "")
            .as_str()
        {
            "who" => Axis::Who,
            "what" => Axis::What,
            "when" => Axis::When,
            "where" => Axis::Where,
            "why" => Axis::Why,
            "how" => Axis::How,
            "outofscope" => Axis::OutOfScope,
            "criticalpath" => Axis::CriticalPath,
            "systemboundaries" => Axis::SystemBoundaries,
            "failurebehavior" => Axis::FailureBehavior,
            "dataownership" => Axis::DataOwnership,
            "authorization" => Axis::Authorization,
            "measurablesuccess" => Axis::MeasurableSuccess,
            "testcases" => Axis::TestCases,
            _ => return None,
        },
    )
}
