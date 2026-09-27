//! Stable identity handling for feature specifications.
use crate::domain::ArtifactIdentity;
use std::path::Path;

pub(crate) fn preserve_feature_identity(
    path: &Path,
    feature_id: &str,
    content: &str,
) -> anyhow::Result<String> {
    let title = content
        .lines()
        .find_map(|line| line.strip_prefix(&format!("# {feature_id}: ")))
        .ok_or_else(|| anyhow::anyhow!("Feature title missing"))?;
    let previous = match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "Feature specification must be a regular file"
            );
            Some(std::fs::read_to_string(path)?)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    ArtifactIdentity::preserve_markdown(content, previous.as_deref(), feature_id, title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_uid_survives_a_real_directory_move_and_rename() {
        let root = std::env::temp_dir().join(format!(
            "packet_feature_identity_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let old_dir = root.join("F10-cache-responses");
        let new_dir = root.join("F10-reuse-cached-responses");
        std::fs::create_dir_all(&old_dir).unwrap();
        let old_path = old_dir.join("specification.md");
        let original = "# F10: Cache responses\n\n## Intent\n\nAvoid duplicate work.\n";
        let created = preserve_feature_identity(&old_path, "F10", original).unwrap();
        std::fs::write(&old_path, &created).unwrap();
        let before = ArtifactIdentity::from_markdown(&created).unwrap().unwrap();
        std::fs::rename(&old_dir, &new_dir).unwrap();

        let new_path = new_dir.join("specification.md");
        let renamed = "# F10: Reuse cached responses\n\n## Intent\n\nAvoid duplicate work.\n";
        let updated = preserve_feature_identity(&new_path, "F10", renamed).unwrap();
        let after = ArtifactIdentity::from_markdown(&updated).unwrap().unwrap();
        assert_eq!(after.uid, before.uid);
        assert_eq!(after.display_id, "F10");
        assert_eq!(after.title, "Reuse cached responses");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn structured_status_survives_title_and_directory_rename() {
        let root = std::env::temp_dir().join(format!(
            "packet_feature_status_identity_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let old_dir = root.join("F10-cache-responses");
        let new_dir = root.join("F10-reuse-cached-responses");
        std::fs::create_dir_all(&old_dir).unwrap();
        let old_path = old_dir.join("specification.md");
        let original =
            "# F10: Cache responses\n\n**Status:** Ready\n\n## Intent\n\nAvoid duplicate work.\n";
        let identified = preserve_feature_identity(&old_path, "F10", original).unwrap();
        let identity = ArtifactIdentity::from_markdown(&identified)
            .unwrap()
            .unwrap();
        let original = crate::domain::ChangeMetadata::write_markdown(
            &identified,
            &identity,
            crate::domain::ChangeStatus::Ready,
        )
        .unwrap();
        std::fs::write(&old_path, &original).unwrap();
        let before = crate::domain::ChangeMetadata::require_markdown(&original).unwrap();
        std::fs::rename(&old_dir, &new_dir).unwrap();

        let new_path = new_dir.join("specification.md");
        let renamed = "# F10: Reuse cached responses\n\n**Status:** Draft\n\n## Intent\n\nAvoid duplicate work.\n";
        let updated = preserve_feature_identity(&new_path, "F10", renamed).unwrap();
        let after_identity = ArtifactIdentity::from_markdown(&updated).unwrap().unwrap();
        let updated =
            crate::domain::ChangeMetadata::write_markdown(&updated, &after_identity, before.status)
                .unwrap();
        let after = crate::domain::ChangeMetadata::require_markdown(&updated).unwrap();
        assert_eq!(after.uid, before.uid);
        assert_eq!(after.status, crate::domain::ChangeStatus::Ready);
        assert!(updated.contains("**Status:** Ready"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
