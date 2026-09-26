use std::{collections::BTreeMap, path::Path};

use crate::{
    artifacts::layout::canonical,
    domain::{ArtifactIdentity, OpenItem},
};

use super::super::plan::Plan as ArtifactPlan;
use super::{Change, change, features::unique_feature_uid, read, register};

pub(super) fn build(
    repo: &Path,
    migration: &ArtifactPlan,
    features: &BTreeMap<String, Vec<ArtifactIdentity>>,
    seen: &mut BTreeMap<String, String>,
) -> anyhow::Result<Vec<Change>> {
    let mut changes = Vec::new();
    if let Some(markdown) = read(repo, migration, canonical::OPEN_ITEMS)? {
        let mut items = crate::artifacts::items_io::parse(&markdown)
            .map_err(|error| anyhow::anyhow!("Cannot seed open-item identities: {error}"))?;
        if !items.is_empty() {
            stabilize(&mut items, features, seen)?;
            let serialized = crate::artifacts::items_io::serialize(&items);
            change(
                &mut changes,
                canonical::OPEN_ITEMS.into(),
                &markdown,
                serialized,
            );
        }
    }
    if let Some(json) = read(repo, migration, canonical::RESOLVED_ITEMS)? {
        let mut items = serde_json::from_str::<Vec<OpenItem>>(&json)
            .map_err(|error| anyhow::anyhow!("Cannot seed resolved-item identities: {error}"))?;
        if !items.is_empty() {
            stabilize(&mut items, features, seen)?;
            let serialized = serde_json::to_string_pretty(&items)?;
            change(
                &mut changes,
                canonical::RESOLVED_ITEMS.into(),
                &json,
                serialized,
            );
        }
    }
    Ok(changes)
}

fn stabilize(
    items: &mut [OpenItem],
    features: &BTreeMap<String, Vec<ArtifactIdentity>>,
    seen: &mut BTreeMap<String, String>,
) -> anyhow::Result<()> {
    for item in items {
        let uid = match item.uid.take() {
            Some(uid) => uuid::Uuid::parse_str(&uid)
                .map_err(|_| anyhow::anyhow!("Open item {} has an invalid UID", item.id))?
                .hyphenated()
                .to_string(),
            None => uuid::Uuid::new_v4().hyphenated().to_string(),
        };
        if let Some(feature_uid) = &item.feature_uid {
            item.feature_uid = Some(
                uuid::Uuid::parse_str(feature_uid)
                    .map_err(|_| {
                        anyhow::anyhow!("Open item {} has an invalid feature UID", item.id)
                    })?
                    .hyphenated()
                    .to_string(),
            );
        }
        let entity = format!("open-item:{}", item.id);
        register(
            seen,
            &ArtifactIdentity {
                uid: uid.clone(),
                display_id: item.id.clone(),
                title: item.question.clone(),
                parent_uid: None,
            },
            &entity,
        )?;
        item.uid = Some(uid);

        let Some(feature_id) = &item.feature_id else {
            continue;
        };
        let Some(feature_uid) = unique_feature_uid(features, feature_id)? else {
            continue;
        };
        anyhow::ensure!(
            item.feature_uid
                .as_deref()
                .is_none_or(|previous| previous == feature_uid),
            "Open item {} links to a different feature identity",
            item.id
        );
        item.feature_uid = Some(feature_uid);
    }
    Ok(())
}
