use std::path::{Path, PathBuf};

/// Advisory location only. Creation still uses no-replace publication because
/// another process may occupy the path after this review.
pub(super) fn suggest(parent: &Path, title: &str) -> Result<PathBuf, String> {
    let slug: String = title
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(100)
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        return Err("Use at least one letter or number in the project name.".into());
    }
    for suffix in 0..1000 {
        let name = if suffix == 0 {
            slug.to_owned()
        } else {
            format!("{slug}-{}", suffix + 1)
        };
        let candidate = parent.join(name);
        match std::fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(candidate),
            Err(_) => {
                return Err(
                    "This parent folder cannot be inspected. Choose an accessible location.".into(),
                );
            }
            Ok(_) => {}
        }
    }
    Err("Choose another project name or parent folder; matching folders already exist.".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn occupied_paths_are_preserved_and_suggestion_is_only_advisory() {
        // Real filesystem collision boundary; temporary directory only. A
        // string-slug unit test cannot prove occupied files are respected.
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("my-app"), b"sentinel").unwrap();
        std::fs::create_dir(root.path().join("my-app-2")).unwrap();
        assert_eq!(
            super::suggest(root.path(), "My App").unwrap(),
            root.path().join("my-app-3")
        );
        assert_eq!(
            std::fs::read(root.path().join("my-app")).unwrap(),
            b"sentinel"
        );
    }
}
