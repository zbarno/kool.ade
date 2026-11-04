use super::*;
use crate::{app::session::Project, core::published_specs::PublishedSpecReader};

/// Publish only the exact feature snapshot approved by independent review.
#[cfg(not(test))]
pub(crate) fn publish(
    project: &mut Project,
    feature_id: &str,
    feature_uid: &str,
    snapshot_hash: &str,
    markdown: &str,
) -> anyhow::Result<String> {
    publish_using(
        project,
        feature_id,
        feature_uid,
        snapshot_hash,
        markdown,
        |root, repository_id, pin, changes, consent| {
            crate::core::spec_publication::publish_approved(
                root,
                repository_id,
                pin,
                changes,
                consent,
            )
        },
    )
}

/// Unit tests model the protected merge by staging the same consented document
/// set and validating it through the fixture merge path. The resulting pin is
/// refreshed from the fixture's code Git repository before callers can proceed.
#[cfg(test)]
pub(crate) fn publish(
    project: &mut Project,
    feature_id: &str,
    feature_uid: &str,
    snapshot_hash: &str,
    markdown: &str,
) -> anyhow::Result<String> {
    super::stage::stage(project, feature_id, feature_uid, snapshot_hash, markdown)?;
    super::simulate::simulate_merged_feature(project, feature_uid, snapshot_hash)?;
    Ok("https://example.invalid/pull/1".into())
}

#[cfg_attr(test, allow(dead_code))]
fn publish_using(
    project: &mut Project,
    feature_id: &str,
    feature_uid: &str,
    snapshot_hash: &str,
    markdown: &str,
    publisher: impl FnOnce(
        &std::path::Path,
        &str,
        &crate::core::published_specs::PublishedSpecPin,
        &[crate::core::spec_publication::DocumentationChange],
        &crate::core::spec_publication::PublicationConsent,
    ) -> anyhow::Result<crate::core::spec_publication::PullRequest>,
) -> anyhow::Result<String> {
    validate_identity(feature_uid, snapshot_hash)?;
    let (receipt_path, pending_path) = paths(feature_uid, snapshot_hash);
    if let Ok(bytes) = project.state.planning_store.read(&receipt_path) {
        let receipt: FeaturePublicationReceipt = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(
            receipt.schema_version == 1,
            "unsupported feature publication receipt"
        );
        return Ok(receipt.pull_request_url);
    }
    let pending: PendingFeaturePublication =
        serde_json::from_slice(&project.state.planning_store.read(&pending_path)?)?;
    let pin = project
        .state
        .published_spec_pin
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No published-spec baseline is pinned"))?;
    anyhow::ensure!(
        pending.published_pin == *pin,
        "published-spec pin changed after feature publication consent; review the new baseline"
    );
    let reader = PublishedSpecReader::new(
        &project.state.repo_root,
        pending.published_pin.repository_id.clone(),
        pending.published_pin.reference.clone(),
    )?;
    validate_pending_changes(
        &pending,
        feature_id,
        feature_uid,
        snapshot_hash,
        markdown,
        &reader,
        pin,
    )?;
    let snapshot = reviewed_snapshot(project, feature_id, feature_uid, markdown)?;
    anyhow::ensure!(
        snapshot.content_hash == snapshot_hash,
        "current feature no longer matches the reviewed snapshot"
    );
    let pr = validate_document_set(
        &project.state.repo_root,
        &pending.published_pin.repository_id,
        &pending.published_pin,
        &pending.changes,
        publisher,
    )?;
    let receipt = FeaturePublicationReceipt {
        schema_version: 1,
        pull_request_url: pr.url.clone(),
        branch: pr.branch,
        base_commit: pr.base_commit,
        head_commit: pr.head_commit,
    };
    project
        .state
        .planning_store
        .atomic_write(&receipt_path, &serde_json::to_vec_pretty(&receipt)?)?;
    project.state.planning_store.remove(&pending_path)?;
    Ok(pr.url)
}

#[cfg(test)]
pub(in crate::app::root::feature_approval) fn publish_document_set_with_test_injection(
    repository_root: &std::path::Path,
    repository_id: &str,
    pin: &crate::core::published_specs::PublishedSpecPin,
    changes: &[crate::core::spec_publication::DocumentationChange],
    publisher: impl FnOnce(
        &std::path::Path,
        &str,
        &crate::core::published_specs::PublishedSpecPin,
        &[crate::core::spec_publication::DocumentationChange],
        &crate::core::spec_publication::PublicationConsent,
    ) -> anyhow::Result<crate::core::spec_publication::PullRequest>,
) -> anyhow::Result<crate::core::spec_publication::PullRequest> {
    validate_document_set(repository_root, repository_id, pin, changes, publisher)
}

pub(in crate::app::root::feature_approval) fn validate_document_set(
    repository_root: &std::path::Path,
    repository_id: &str,
    pin: &crate::core::published_specs::PublishedSpecPin,
    changes: &[crate::core::spec_publication::DocumentationChange],
    publisher: impl FnOnce(
        &std::path::Path,
        &str,
        &crate::core::published_specs::PublishedSpecPin,
        &[crate::core::spec_publication::DocumentationChange],
        &crate::core::spec_publication::PublicationConsent,
    ) -> anyhow::Result<crate::core::spec_publication::PullRequest>,
) -> anyhow::Result<crate::core::spec_publication::PullRequest> {
    crate::core::spec_publication::validate_changes(changes)?;
    let consent = crate::core::spec_publication::PublicationConsent::confirmed(changes)?;
    publisher(repository_root, repository_id, pin, changes, &consent)
}
