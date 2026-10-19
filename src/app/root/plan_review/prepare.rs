use super::PlanReviewOutcome;
pub(super) fn recover_persisted_review(
    project: &super::super::session::Project,
    feature_id: &str,
) -> Result<Option<PlanReviewOutcome>, String> {
    let text = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == feature_id)
        .map(|(_, text)| text.as_str())
        .ok_or_else(|| "confirmed feature is no longer active".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(text)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "feature identity is missing".to_owned())?;
    let contract = crate::core::workflow::feature_contract(text);
    let store = &project.state.planning_store;
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        store, feature_id, &identity, text, &contract,
    )
    .map_err(|error| error.to_string())?;
    let Some(receipt) =
        crate::core::lifecycle::plan_snapshot::plan_review_receipt(store, &snapshot)
            .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let Some(verdict) =
        crate::core::lifecycle::plan_snapshot::plan_review_verdict(store, &snapshot)
            .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    if !crate::core::lifecycle::plan_snapshot::has_current_user_confirmation(store, &snapshot)
        .map_err(|error| error.to_string())?
    {
        return Err("saved plan review has no current matching user confirmation".into());
    }
    let requirements = snapshot
        .requirements
        .iter()
        .map(|requirement| requirement.stable_id.clone())
        .collect();
    Ok(Some(PlanReviewOutcome {
        feature_id: feature_id.to_owned(),
        snapshot_uid: snapshot.uid,
        snapshot_hash: snapshot.content_hash,
        author_run_id: receipt
            .author_run_id
            .clone()
            .ok_or_else(|| "saved plan review has no author run identity".to_owned())?,
        author_session_id: receipt
            .author_session_id
            .clone()
            .ok_or_else(|| "saved plan review has no author session identity".to_owned())?,
        reviewer_run_id: receipt
            .reviewer_run_id
            .clone()
            .ok_or_else(|| "saved plan review has no reviewer run identity".to_owned())?,
        reviewer_session_id: receipt
            .reviewer_session_id
            .clone()
            .ok_or_else(|| "saved plan review has no reviewer session identity".to_owned())?,
        evidence_refs: receipt.evidence_refs.clone(),
        prompt: receipt
            .prompt_manifest
            .clone()
            .ok_or_else(|| "saved plan review has no prompt provenance".to_owned())?,
        verdict,
        review_error: None,
        arbitration: None,
        requirements,
    }))
}
