use super::*;
use crate::{app::session::Project, core::published_specs::PublishedSpecReader};

/// Work can start only when the exact feature is in the currently pinned tree.
pub(crate) fn pin_is_published(project: &Project, feature_uid: &str, markdown: &str) -> bool {
    let Some(pin) = project.state.published_spec_pin.as_ref() else {
        return false;
    };
    let Ok(reader) = PublishedSpecReader::new(
        &project.state.repo_root,
        pin.repository_id.clone(),
        pin.reference.clone(),
    ) else {
        return false;
    };
    reader
        .read(pin, &document_path(feature_uid))
        .is_ok_and(|document| document.content == markdown)
}

pub(in crate::app::root::feature_approval) fn reviewed_snapshot(
    project: &Project,
    feature_id: &str,
    feature_uid: &str,
    markdown: &str,
) -> anyhow::Result<crate::core::lifecycle::PlanSnapshotV1> {
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .map_err(anyhow::Error::msg)?
        .ok_or_else(|| anyhow::anyhow!("feature identity is missing"))?;
    anyhow::ensure!(
        identity.uid == feature_uid,
        "feature UID does not match its document"
    );
    anyhow::ensure!(
        project
            .state
            .active_features
            .iter()
            .any(|(id, content)| id == feature_id && content == markdown),
        "feature document is not the current active feature"
    );
    crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        feature_id,
        &identity,
        markdown,
        &crate::core::workflow::feature_contract(markdown),
    )
    .map_err(anyhow::Error::msg)
}

pub(in crate::app::root::feature_approval) fn validate_pending_changes(
    pending: &PendingFeaturePublication,
    feature_id: &str,
    feature_uid: &str,
    snapshot_hash: &str,
    markdown: &str,
    reader: &PublishedSpecReader,
    current_pin: &crate::core::published_specs::PublishedSpecPin,
) -> anyhow::Result<()> {
    validate_identity(feature_uid, snapshot_hash)?;
    anyhow::ensure!(
        pending.schema_version == 1
            && pending.feature_id == feature_id
            && pending.feature_uid == feature_uid
            && pending.feature_snapshot_sha256 == snapshot_hash
            && pending.feature_markdown_sha256 == sha256(markdown),
        "pending feature publication does not bind the reviewed document"
    );
    anyhow::ensure!(
        &pending.published_pin == current_pin,
        "pending feature publication baseline changed"
    );
    reader.ensure_local_ref_unchanged(&pending.published_pin)?;
    crate::core::spec_publication::validate_changes(&pending.changes)?;
    let expected_feature_path = document_path(feature_uid);
    let index_path = "docs/koolade/features/AGENTS.md";
    let root_index_path = "docs/koolade/AGENTS.md";
    anyhow::ensure!(
        pending.changes.len() == 3,
        "pending feature publication has unexpected documents"
    );
    let feature = pending
        .changes
        .iter()
        .find(|change| change.path == expected_feature_path)
        .ok_or_else(|| anyhow::anyhow!("pending feature document is missing"))?;
    anyhow::ensure!(
        feature.content == markdown,
        "pending feature document was changed after consent"
    );
    let baseline_index = reader
        .read_optional(&pending.published_pin, index_path)?
        .map_or_else(|| "# Navigation\n".to_owned(), |document| document.content);
    let expected_index = add_navigation_link(&baseline_index, feature_uid)?;
    let index = pending
        .changes
        .iter()
        .find(|change| change.path == index_path)
        .ok_or_else(|| anyhow::anyhow!("pending navigation index is missing"))?;
    anyhow::ensure!(
        index.content == expected_index,
        "pending navigation index delta was changed after consent"
    );
    let baseline_root_index = reader
        .read_optional(&pending.published_pin, root_index_path)?
        .map_or_else(|| "# Navigation\n".to_owned(), |document| document.content);
    let expected_root_index = add_navigation_entry(
        &baseline_root_index,
        "features/AGENTS.md",
        "./features/AGENTS.md",
        "Published feature specifications.",
    )?;
    let root_index = pending
        .changes
        .iter()
        .find(|change| change.path == root_index_path)
        .ok_or_else(|| anyhow::anyhow!("pending root navigation index is missing"))?;
    anyhow::ensure!(
        root_index.content == expected_root_index,
        "pending root navigation index delta was changed after consent"
    );
    let consent = crate::core::spec_publication::PublicationConsent::confirmed(&pending.changes)?;
    anyhow::ensure!(
        consent.digest() == pending.consent_sha256,
        "pending consent binding failed"
    );
    Ok(())
}

pub(in crate::app::root::feature_approval) fn sha256(text: &str) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(text.as_bytes()))
}

pub(in crate::app::root::feature_approval) fn validate_identity(
    uid: &str,
    hash: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(uuid::Uuid::parse_str(uid).is_ok(), "feature UID is invalid");
    anyhow::ensure!(
        hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "feature snapshot identity is invalid"
    );
    Ok(())
}
