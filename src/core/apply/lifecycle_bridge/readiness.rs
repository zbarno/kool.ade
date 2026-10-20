use crate::core::state::PlannerState;

pub(crate) fn readiness_record_check(
    state: &PlannerState,
) -> anyhow::Result<Option<crate::artifacts::planning_store::RecordRevisionCheck>> {
    let Some((feature_id, markdown)) = state.active_feature.as_ref() else {
        return Ok(None);
    };
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)?
        .ok_or_else(|| anyhow::anyhow!("active feature has no stable identity"))?;
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&state.planning_store);
    match lifecycle.load_feature(&identity.uid) {
        Ok(feature) if feature.plan_snapshot_uid.is_some() => {}
        Ok(_) => return Ok(None),
        Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    }
    let contract = crate::core::workflow::feature_contract(markdown);
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &state.planning_store,
        feature_id,
        &identity,
        markdown,
        &contract,
    )
    .map_err(anyhow::Error::msg)?;
    let check = crate::core::lifecycle::plan_snapshot::require_current_readiness(
        &state.planning_store,
        &snapshot,
    )
    .map_err(anyhow::Error::msg)?;
    Ok(Some(check))
}
