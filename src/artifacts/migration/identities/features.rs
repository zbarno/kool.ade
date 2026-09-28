use std::{collections::BTreeMap, path::Path};

use crate::{
    artifacts::layout::canonical,
    domain::{ArtifactIdentity, ChangeMetadata},
};

use super::super::plan::Plan as ArtifactPlan;
use super::{Change, change, files_under, preserve_markdown, register};

type MigratedFeatures = (BTreeMap<String, Vec<ArtifactIdentity>>, Vec<Change>);

pub(super) fn build(
    repo: &Path,
    migration: &ArtifactPlan,
    seen: &mut BTreeMap<String, String>,
) -> anyhow::Result<MigratedFeatures> {
    let files = files_under(repo, migration, canonical::CHANGES)?;
    let mut identities = BTreeMap::<String, Vec<ArtifactIdentity>>::new();
    let mut changes = Vec::new();
    for (path, markdown) in files {
        if !path.ends_with("/specification.md") {
            continue;
        }
        let Some((feature_id, title)) = feature_title(&markdown) else {
            // Historical directories may contain notes that are not features.
            continue;
        };
        let existing_metadata = ChangeMetadata::from_markdown(&markdown)?;
        let clean_markdown = ChangeMetadata::strip_markers(&markdown);
        let existing = ArtifactIdentity::from_markdown(&clean_markdown)?;
        if let Some(identity) = &existing {
            anyhow::ensure!(
                identity.display_id == feature_id,
                "Feature identity at {path} conflicts with its heading ID {feature_id}"
            );
        }
        let (contents, identity) = preserve_markdown(&clean_markdown, feature_id, title, None)?;
        let status = match &existing_metadata {
            Some(metadata) => {
                anyhow::ensure!(
                    metadata.uid == identity.uid && metadata.display_id == identity.display_id,
                    "Change status identity at {path} conflicts with its artifact identity"
                );
                metadata.status
            }
            None => ChangeMetadata::parse_legacy_markdown(&markdown).map_err(|error| {
                anyhow::anyhow!("Cannot migrate change status at {path}: {error}")
            })?,
        };
        let contents = ChangeMetadata::rewrite_markdown(
            &contents,
            &identity,
            status,
            existing_metadata.as_ref(),
        )?;
        register(seen, &identity, &format!("feature:{path}"))?;
        identities
            .entry(feature_id.to_owned())
            .or_default()
            .push(identity);
        change(&mut changes, path, &markdown, contents);
    }
    Ok((identities, changes))
}

fn feature_title(markdown: &str) -> Option<(&str, &str)> {
    let heading = markdown.lines().find_map(|line| line.strip_prefix("# "))?;
    let (id, title) = heading.split_once(": ")?;
    crate::artifacts::product_docs::valid_feature_id(id)
        .then_some((id, title.trim()))
        .filter(|(_, title)| !title.is_empty())
}

pub(super) fn unique_feature_uid(
    features: &BTreeMap<String, Vec<ArtifactIdentity>>,
    feature_id: &str,
) -> anyhow::Result<Option<String>> {
    match features
        .get(feature_id)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        [] => Ok(None),
        [identity] => Ok(Some(identity.uid.clone())),
        duplicates => anyhow::bail!(
            "Feature ID {feature_id} identifies {} historical records; resolve the duplicate before linking tasks or planning items",
            duplicates.len()
        ),
    }
}
