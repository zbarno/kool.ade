use super::ProjectManifest;
use std::path::Path;

/// Persist only repository display-name edits, preserving all stable identity
/// and routing fields from the connected manifest.
pub fn save_display_names(root: &Path, mut updated: ProjectManifest) -> anyhow::Result<()> {
    let current = ProjectManifest::load(root)?;
    updated.normalize_display_names()?;
    updated.validate()?;
    anyhow::ensure!(
        current.repositories.len() == updated.repositories.len(),
        "Repository display-name save cannot add or remove repositories"
    );
    for (before, after) in current.repositories.iter().zip(&updated.repositories) {
        anyhow::ensure!(
            before.id == after.id && before.role == after.role && before.remote == after.remote,
            "Repository display-name save cannot change repository identity or role"
        );
    }
    let bytes = serde_json::to_vec_pretty(&updated)?;
    let path = crate::artifacts::layout::ArtifactLayout::new(root).project_manifest();
    crate::artifacts::atomic_write(&path, std::str::from_utf8(&bytes)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "koolade-repo-name-save-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(root.join(".koolade-packet/config")).unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap();
        let manifest = ProjectManifest {
            repositories: vec![crate::core::project_repos::Repository {
                id: "root".into(),
                role: "Planning root".into(),
                remote: String::new(),
                display_name: None,
            }],
        };
        std::fs::write(
            crate::artifacts::layout::ArtifactLayout::new(&root).project_manifest(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        root
    }

    #[test]
    fn name_save_trims_and_keeps_identity_fields_unchanged() {
        let root = fixture();
        let mut manifest = ProjectManifest::load(&root).unwrap();
        manifest.repositories[0].display_name = Some("  Product  ".into());
        save_display_names(&root, manifest).unwrap();
        let saved = ProjectManifest::load(&root).unwrap();
        assert_eq!(
            saved.repositories[0].display_name.as_deref(),
            Some("Product")
        );
        assert_eq!(saved.repositories[0].id, "root");
        assert_eq!(saved.repositories[0].role, "Planning root");
        assert!(
            std::fs::read_to_string(
                crate::artifacts::layout::ArtifactLayout::new(&root).project_manifest()
            )
            .unwrap()
            .contains("Product")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_name_save_leaves_manifest_bytes_untouched() {
        let root = fixture();
        let path = crate::artifacts::layout::ArtifactLayout::new(&root).project_manifest();
        let before = std::fs::read(&path).unwrap();
        let mut manifest = ProjectManifest::load(&root).unwrap();
        manifest.repositories[0].display_name = Some("x".repeat(41));
        assert!(save_display_names(&root, manifest).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(root);
    }
}
