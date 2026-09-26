//! Validation for manifest-owned logical module identities and paths.
use std::path::{Component, Path, PathBuf};

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id != "index"
        && id != "manifest"
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

pub fn safe_module_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    if path.components().count() != 1
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(root.join(path))
}
