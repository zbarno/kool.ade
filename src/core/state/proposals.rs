pub(crate) fn apply_local_feature_proposals(
    features: &mut Vec<(String, String)>,
    draft: Option<&crate::core::apply::LocalSpecDraftV1>,
    current_pin: Option<&crate::core::published_specs::PublishedSpecPin>,
) -> anyhow::Result<()> {
    let Some(draft) = draft else {
        return Ok(());
    };
    // A stale proposal remains available for review in its local record, but
    // it cannot affect current planning context until it is re-reviewed.
    if draft.base_published_spec_pin.as_ref() != current_pin {
        return Ok(());
    }
    for change in &draft.changes {
        let Some(feature_id) = change.document_id.strip_prefix("feature:") else {
            continue;
        };
        crate::core::specification::validate_feature(feature_id, &change.content)?;
        let identity = crate::domain::ArtifactIdentity::from_markdown(&change.content)?
            .ok_or_else(|| anyhow::anyhow!("local feature proposal has no stable identity"))?;
        anyhow::ensure!(
            identity.display_id == feature_id,
            "local feature proposal identity does not match its document ID"
        );
        let metadata = crate::domain::ChangeMetadata::require_markdown(&change.content)?;
        anyhow::ensure!(
            metadata.uid == identity.uid && metadata.display_id == identity.display_id,
            "local feature proposal metadata does not match its identity"
        );
        if let Some(index) = features.iter().position(|(id, _)| id == feature_id) {
            features.remove(index);
        }
        if !metadata.status.is_terminal() {
            features.push((
                feature_id.to_owned(),
                crate::domain::ChangeMetadata::render_status(&change.content, metadata.status)?,
            ));
        }
    }
    features.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(())
}
