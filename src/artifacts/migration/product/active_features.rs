use std::fs;

use crate::artifacts::planning_store::{PlanningLayout, PlanningStore, StoreMode};

pub(super) fn append_active_features(
    store: &PlanningStore,
    layout: &PlanningLayout,
    index: &mut String,
) -> anyhow::Result<()> {
    let legacy_features = layout.legacy_features_root();
    let changes = if store.mode == StoreMode::LegacyEmbedded && legacy_features.is_dir() {
        legacy_features
    } else {
        layout.changes_root()
    };
    let mut features = Vec::new();
    if changes.is_dir() {
        for entry in fs::read_dir(changes)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Feature entry {} is not a real directory",
                entry.path().display()
            );
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Feature directory name is not valid UTF-8"))?;
            let feature = entry.path().join("specification.md");
            let text = if feature.starts_with(layout.root()) {
                store
                    .read_planning_path(&feature)
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
            } else {
                fs::read_to_string(&feature).ok()
            };
            let Some(text) = text else {
                continue;
            };
            let Some((id, title)) = text
                .lines()
                .find_map(|line| line.strip_prefix("# "))
                .and_then(|heading| heading.split_once(": "))
            else {
                continue;
            };
            if !crate::artifacts::product_docs::valid_feature_id(id) || title.trim().is_empty() {
                continue;
            }
            let status = crate::domain::ChangeMetadata::from_markdown(&text)?
                .map(|metadata| metadata.status)
                .map_or_else(
                    || crate::domain::ChangeMetadata::parse_legacy_markdown(&text),
                    Ok,
                )?;
            if !status.is_terminal() {
                features.push(name);
            }
        }
    }
    features.sort();
    if features.is_empty() {
        index.push_str("None.\n");
    } else {
        for feature in features {
            index.push_str(&format!(
                "- [`{feature}`](../changes/{feature}/specification.md)\n"
            ));
        }
    }
    Ok(())
}
