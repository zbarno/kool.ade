use std::{fs, path::Path};

pub(super) fn safe_target(root: &Path, target: &Path) -> anyhow::Result<bool> {
    let relative = target.strip_prefix(root)?;
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return Ok(false);
        };
        cursor.push(part);
        if fs::symlink_metadata(&cursor).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn sync_path(path: &str) -> bool {
    let relative = Path::new(path);
    let safe = !path.is_empty()
        && relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)));
    safe && (path.starts_with("state/") || path.starts_with("planning/"))
        && !matches!(
            path,
            "state/work.json"
                | "state/workflow.json"
                | "planning/open-items.md"
                | "planning/resolved-items.json"
        )
}
