use crate::{core::state::PlannerState, error::AppError};

pub(super) fn approved_snapshot(
    state: &PlannerState,
) -> Result<Option<crate::core::lifecycle::PlanSnapshotV1>, AppError> {
    let Some((feature_id, markdown)) = state.active_feature.as_ref() else {
        return Ok(None);
    };
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .map_err(|error| AppError::Other(error.to_string()))?;
    let Some(identity) = identity else {
        return Ok(None);
    };
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&state.planning_store);
    let goal = match lifecycle.load_feature(&identity.uid) {
        Ok(goal) => goal,
        Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(None);
        }
        Err(error) => return Err(AppError::Other(error.to_string())),
    };
    if goal.plan_snapshot_uid.is_none() {
        return Err(AppError::Other(
            "Lifecycle task generation requires a persisted approved plan snapshot.".into(),
        ));
    }
    if goal.phase != crate::core::lifecycle::FeaturePhase::ReadyToDecompose {
        return Err(AppError::Other(
            "Lifecycle task generation requires a feature ready for decomposition.".into(),
        ));
    }
    if !crate::core::workflow::feature_approved(&state.planning_store, &state.workflow, feature_id)
    {
        return Err(AppError::Other(
            "Task decomposition requires explicit user approval and an independent review of the current plan.".into(),
        ));
    }
    let contract = crate::core::workflow::feature_contract(markdown);
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &state.planning_store,
        feature_id,
        &identity,
        markdown,
        &contract,
    )
    .map_err(AppError::Other)?;
    crate::core::lifecycle::plan_snapshot::current_readiness(&state.planning_store, &snapshot)
        .map_err(|error| AppError::Other(format!("Current plan readiness is stale: {error}")))?;
    let goal_matches = goal.plan_snapshot_uid.as_deref() == Some(snapshot.uid.as_str())
        && goal.plan_snapshot_sha256.as_deref() == Some(snapshot.content_hash.as_str())
        && goal.approved_plan_sha256.as_deref() == Some(snapshot.content_hash.as_str());
    let snapshot_persisted =
        crate::core::lifecycle::plan_snapshot::is_persisted(&state.planning_store, &snapshot)
            .map_err(|error| AppError::Other(error.to_string()))?;
    let confirmed = crate::core::lifecycle::plan_snapshot::has_current_user_confirmation(
        &state.planning_store,
        &snapshot,
    )
    .map_err(|error| AppError::Other(error.to_string()))?;
    let reviewed = crate::core::lifecycle::plan_snapshot::has_approved_review(
        &state.planning_store,
        &snapshot,
    )
    .map_err(|error| AppError::Other(error.to_string()))?;
    if !goal_matches || !snapshot_persisted || !confirmed || !reviewed {
        return Err(AppError::Other(format!(
            "The approved plan snapshot or its confirmation/review receipts are stale (feature={feature_id}, feature_uid={}, snapshot_uid={}, plan_hash={}, goal={goal_matches}, snapshot={snapshot_persisted}, confirmation={confirmed}, review={reviewed}).",
            snapshot.feature_uid, snapshot.uid, snapshot.content_hash
        )));
    }
    Ok(Some(snapshot))
}
