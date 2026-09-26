use super::feature_index::directory_feature_id;
use std::path::Path;

pub fn active_feature(repo: &Path) -> Option<(String, String)> {
    active_features(repo).into_iter().next()
}

/// Every feature that is still active. Feature status is independent, so
/// several deltas may be planned or implemented at the same time.
pub fn active_features(repo: &Path) -> Vec<(String, String)> {
    let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
    active_feature_directories(repo)
        .into_iter()
        .filter_map(|name| {
            let id = directory_feature_id(&name)?.to_string();
            let body = std::fs::read_to_string(layout.change_specification(&name)?).ok()?;
            Some((id, body))
        })
        .collect()
}

pub(super) fn active_feature_directories(repo: &Path) -> Vec<String> {
    let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
    let Ok(entries) = std::fs::read_dir(layout.changes_root()) else {
        return Vec::new();
    };
    let mut entries = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let path = layout.change_specification(&name)?;
            let body = std::fs::read_to_string(path).ok()?;
            if body.contains("**Status:** Implemented") || body.contains("**Status:** Abandoned") {
                return None;
            }
            Some(name)
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}
