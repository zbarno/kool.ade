use super::*;
use crate::{app::session::Project, core::published_specs::PublishedSpecReader};

/// Save the exact proposal after the user confirms its public disclosure.
pub(crate) fn stage(
    project: &Project,
    feature_id: &str,
    feature_uid: &str,
    snapshot_hash: &str,
    markdown: &str,
) -> anyhow::Result<()> {
    validate_identity(feature_uid, snapshot_hash)?;
    let snapshot = reviewed_snapshot(project, feature_id, feature_uid, markdown)?;
    anyhow::ensure!(
        snapshot.content_hash == snapshot_hash,
        "feature content does not match the reviewed plan snapshot"
    );
    let pin = project
        .state
        .published_spec_pin
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No published-spec baseline is pinned"))?;
    let reader = PublishedSpecReader::new(
        &project.state.repo_root,
        pin.repository_id.clone(),
        pin.reference.clone(),
    )?;
    let index_path = "docs/koolade/features/AGENTS.md";
    let root_index_path = "docs/koolade/AGENTS.md";
    let index = reader
        .read_optional(pin, index_path)?
        .map_or_else(|| "# Navigation\n".to_owned(), |document| document.content);
    let root_index = reader
        .read_optional(pin, root_index_path)?
        .map_or_else(|| "# Navigation\n".to_owned(), |document| document.content);
    let changes = vec![
        crate::core::spec_publication::DocumentationChange {
            path: document_path(feature_uid),
            content: markdown.to_owned(),
        },
        crate::core::spec_publication::DocumentationChange {
            path: index_path.to_owned(),
            content: add_navigation_link(&index, feature_uid)?,
        },
        crate::core::spec_publication::DocumentationChange {
            path: root_index_path.to_owned(),
            content: add_navigation_entry(
                &root_index,
                "features/AGENTS.md",
                "./features/AGENTS.md",
                "Published feature specifications.",
            )?,
        },
    ];
    let consent = crate::core::spec_publication::PublicationConsent::confirmed(&changes)?;
    let record = PendingFeaturePublication {
        schema_version: 1,
        feature_id: feature_id.to_owned(),
        feature_uid: feature_uid.to_owned(),
        feature_snapshot_sha256: snapshot_hash.to_owned(),
        feature_markdown_sha256: sha256(markdown),
        published_pin: pin.clone(),
        consent_sha256: consent.digest().to_owned(),
        changes,
    };
    let (_, pending) = paths(feature_uid, snapshot_hash);
    project
        .state
        .planning_store
        .atomic_write(pending, &serde_json::to_vec_pretty(&record)?)?;
    Ok(())
}
