//! Koolade-owned identity metadata for generated task stories.
use crate::domain::ArtifactIdentity;
use std::path::Path;

pub(super) fn new_batch_identity(title: &str) -> ArtifactIdentity {
    let mut identity = ArtifactIdentity::new("pending", title);
    identity.display_id = format!("BATCH-{}", identity.uid[..8].to_ascii_uppercase());
    identity
}

pub(super) fn choose_batch_identity(
    recorded: Option<ArtifactIdentity>,
    document: Option<ArtifactIdentity>,
    title: &str,
) -> anyhow::Result<ArtifactIdentity> {
    if let (Some(recorded), Some(document)) = (&recorded, &document) {
        anyhow::ensure!(
            recorded.uid == document.uid && recorded.display_id == document.display_id,
            "Task batch identity conflicts between workflow checkpoint and batch index"
        );
    }
    let mut identity = recorded
        .or(document)
        .unwrap_or_else(|| new_batch_identity(title));
    identity.title = title.to_owned();
    Ok(identity)
}

pub(super) fn link_batch_to_feature(
    repo: &Path,
    feature_id: Option<&str>,
    batch_identity: &mut ArtifactIdentity,
) -> anyhow::Result<()> {
    let Some(feature_id) = feature_id else {
        return Ok(());
    };
    let Ok(path) =
        crate::artifacts::product_docs::document_path(repo, &format!("feature:{feature_id}"))
    else {
        return Ok(());
    };
    let Some(feature_identity) = ArtifactIdentity::from_markdown(&std::fs::read_to_string(path)?)?
    else {
        return Ok(());
    };
    anyhow::ensure!(
        batch_identity
            .parent_uid
            .as_deref()
            .is_none_or(|parent| parent == feature_identity.uid),
        "Task batch is linked to a different feature identity"
    );
    batch_identity.parent_uid = Some(feature_identity.uid);
    Ok(())
}

pub(super) fn embed_identity(
    markdown: &str,
    identity: &ArtifactIdentity,
) -> anyhow::Result<String> {
    let seed = format!(
        "<!-- koolade-artifact-id:v1 {} -->",
        serde_json::to_string(identity)?
    );
    ArtifactIdentity::preserve_markdown_with_parent(
        markdown,
        Some(&seed),
        &identity.display_id,
        &identity.title,
        identity.parent_uid.as_deref(),
    )
}

pub(super) fn read_identity(path: &Path) -> anyhow::Result<Option<ArtifactIdentity>> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "Task batch index must be a regular file"
            );
            ArtifactIdentity::from_markdown(&std::fs::read_to_string(path)?)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn resolve_batch_directory(
    repo: &Path,
    batch: &crate::core::workflow::TaskBatchRef,
) -> Option<String> {
    let task_root = crate::artifacts::koolade::task_dir(repo);
    let prefix = format!("{task_root}/");
    let safe_directory = |directory: &str| -> Option<String> {
        let name = directory.strip_prefix(&prefix)?;
        (!name.is_empty()
            && name.len() <= 120
            && !name.contains('/')
            && name.chars().all(|c| c.is_alphanumeric() || c == '-'))
        .then(|| name.to_owned())
    };
    if let Some(name) = safe_directory(&batch.directory) {
        let path = repo.join(&batch.directory);
        if std::fs::symlink_metadata(&path)
            .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        {
            return Some(format!("{task_root}/{name}"));
        }
    }
    let identity = batch.identity.as_ref()?;
    let entries = std::fs::read_dir(repo.join(&task_root)).ok()?;
    let mut matches = Vec::new();
    for entry in entries.flatten() {
        if !entry
            .file_type()
            .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
        {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.len() > 120 || !name.chars().all(|c| c.is_alphanumeric() || c == '-') {
            continue;
        }
        let index = entry.path().join("README.md");
        if read_identity(&index)
            .ok()
            .flatten()
            .is_some_and(|candidate| candidate.uid == identity.uid)
        {
            matches.push(format!("{task_root}/{name}"));
        }
    }
    (matches.len() == 1).then(|| matches.remove(0))
}

pub(super) fn with_path_identity(
    path: &Path,
    content: &str,
    suggested_display_id: &str,
    title: &str,
    parent_uid: Option<&str>,
) -> anyhow::Result<String> {
    let previous = match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "Task story must be a regular file"
            );
            Some(std::fs::read_to_string(path)?)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let display_id = previous
        .as_deref()
        .map(ArtifactIdentity::from_markdown)
        .transpose()?
        .flatten()
        .map(|identity| identity.display_id)
        .unwrap_or_else(|| suggested_display_id.to_owned());
    ArtifactIdentity::preserve_markdown_with_parent(
        content,
        previous.as_deref(),
        &display_id,
        title,
        parent_uid,
    )
}

pub(crate) fn visible_content(markdown: &str) -> String {
    super::metadata::visible_content(markdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_uid_survives_file_move_and_title_change() {
        let root = std::env::temp_dir().join(format!(
            "koolade_task_identity_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let old_path = root.join("BATCH-001-TASK-001-original-title.md");
        let original = "# Original title\n\nTask context.\n";
        let created = with_path_identity(
            &old_path,
            original,
            "BATCH-001-TASK-001",
            "Original title",
            None,
        )
        .unwrap();
        std::fs::write(&old_path, &created).unwrap();
        let before = ArtifactIdentity::from_markdown(&created).unwrap().unwrap();
        let new_path = root.join("renamed-task.md");
        std::fs::rename(&old_path, &new_path).unwrap();

        let renamed = "# Renamed task\n\nTask context.\n";
        let updated = with_path_identity(
            &new_path,
            renamed,
            "ignored-new-display-id",
            "Renamed task",
            None,
        )
        .unwrap();
        let after = ArtifactIdentity::from_markdown(&updated).unwrap().unwrap();
        assert_eq!(after.uid, before.uid);
        assert_eq!(after.display_id, before.display_id);
        assert_eq!(after.title, "Renamed task");
        assert_eq!(visible_content(&updated), renamed);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn moved_batch_and_renamed_task_are_resolved_by_uid() {
        let root = std::env::temp_dir().join(format!(
            "koolade_batch_identity_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let task_root = root.join(crate::artifacts::koolade::KOOLADE_TASKS_DIR);
        let old_dir = task_root.join("old-batch-name");
        let new_dir = task_root.join("renamed-batch");
        std::fs::create_dir_all(&old_dir).unwrap();
        let batch_identity = new_batch_identity("Saved searches");
        let index = embed_identity("# Saved searches — task stories\n", &batch_identity).unwrap();
        std::fs::write(old_dir.join("README.md"), index).unwrap();
        let old_task = old_dir.join("001-original-title.md");
        let task_text = with_path_identity(
            &old_task,
            "# Original title\n\nTask context.\n",
            "001-original-title",
            "Original title",
            Some(&batch_identity.uid),
        )
        .unwrap();
        let task_identity = ArtifactIdentity::from_markdown(&task_text)
            .unwrap()
            .unwrap();
        std::fs::write(&old_task, task_text).unwrap();
        std::fs::rename(&old_dir, &new_dir).unwrap();
        std::fs::rename(
            new_dir.join("001-original-title.md"),
            new_dir.join("renamed-task.md"),
        )
        .unwrap();

        let mut workflow = crate::core::workflow::Workflow::default();
        workflow
            .task_batches
            .push(crate::core::workflow::TaskBatchRef {
                identity: Some(batch_identity),
                feature: "Saved searches".into(),
                directory: format!(
                    "{}/old-batch-name",
                    crate::artifacts::koolade::KOOLADE_TASKS_DIR
                ),
                count: 1,
            });
        let documents = crate::artifacts::task_docs::load_board(&root, &workflow);
        assert_eq!(documents.len(), 1);
        assert!(documents[0].path.ends_with("renamed-batch/renamed-task.md"));
        assert_eq!(documents[0].identity.as_ref(), Some(&task_identity));
        assert!(!documents[0].text.contains("koolade-artifact-id"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
