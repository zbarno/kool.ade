//! Stable IDs for generated architectural decision records.
use crate::domain::ArtifactIdentity;
use std::path::Path;

pub(super) fn new_adr_identity(
    directory: &Path,
    title: &str,
    parent_uid: Option<&str>,
) -> ArtifactIdentity {
    let mut identity = ArtifactIdentity::new("ADR-000", title);
    let next = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|text| ArtifactIdentity::from_markdown(&text).ok().flatten())
        .filter_map(|identity| {
            identity
                .display_id
                .strip_prefix("ADR-")?
                .parse::<u32>()
                .ok()
        })
        .max()
        .unwrap_or(0)
        + 1;
    identity.display_id = format!("ADR-{next:03}");
    identity.parent_uid = parent_uid.map(str::to_owned);
    identity
}

pub(super) fn embed(
    body: &str,
    previous: Option<&str>,
    identity: &ArtifactIdentity,
) -> anyhow::Result<String> {
    let seed = format!(
        "<!-- packet-artifact-id:v1 {} -->",
        serde_json::to_string(identity)?
    );
    let trusted_previous = if let Some(previous) = previous {
        if ArtifactIdentity::from_markdown(previous)?.is_some() {
            previous
        } else {
            &seed
        }
    } else {
        &seed
    };
    ArtifactIdentity::preserve_markdown_with_parent(
        body,
        Some(trusted_previous),
        &identity.display_id,
        &identity.title,
        identity.parent_uid.as_deref(),
    )
}
