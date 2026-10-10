//! Koolade-owned identity metadata for generated task stories.
use crate::artifacts::planning_store::PlanningRoot;
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

pub(super) fn link_batch_to_feature<R: PlanningRoot + ?Sized>(
    repo: &R,
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
    let text = String::from_utf8(repo.read_planning_path(&path)?)?;
    let Some(feature_identity) = ArtifactIdentity::from_markdown(&text)? else {
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

pub(super) fn resolve_batch_directory<R: PlanningRoot + ?Sized>(
    repo: &R,
    batch: &crate::core::workflow::TaskBatchRef,
) -> Option<String> {
    let task_root = crate::artifacts::koolade::task_dir(repo);
    let accepted_roots = [
        task_root.as_str(),
        crate::artifacts::planning_store::paths::TASKS,
        crate::artifacts::layout::canonical::TASKS,
    ];
    for root in accepted_roots {
        let Some(name) = batch.directory.strip_prefix(&format!("{root}/")) else {
            continue;
        };
        if name.is_empty()
            || name.len() > 120
            || name.contains('/')
            || !name.chars().all(|c| c.is_alphanumeric() || c == '-')
        {
            continue;
        }
        let path = repo.planning_layout().canonical_path(&batch.directory)?;
        if std::fs::symlink_metadata(&path)
            .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        {
            return Some(format!("{task_root}/{name}"));
        }
    }
    let identity = batch.identity.as_ref()?;
    let entries = std::fs::read_dir(repo.planning_layout().canonical_path(&task_root)?).ok()?;
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
        if read_identity_in(repo, &index)
            .ok()
            .flatten()
            .is_some_and(|candidate| candidate.uid == identity.uid)
        {
            matches.push(format!("{task_root}/{name}"));
        }
    }
    (matches.len() == 1).then(|| matches.remove(0))
}

pub(super) fn read_identity_in<R: PlanningRoot + ?Sized>(
    repo: &R,
    path: &Path,
) -> anyhow::Result<Option<ArtifactIdentity>> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "Task batch index must be a regular file"
            );
            let text = String::from_utf8(repo.read_planning_path(path)?)?;
            ArtifactIdentity::from_markdown(&text)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
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

pub(super) fn with_planning_path_identity<R: PlanningRoot + ?Sized>(
    repo: &R,
    path: &Path,
    content: &str,
    suggested_display_id: &str,
    title: &str,
    parent_uid: Option<&str>,
) -> anyhow::Result<String> {
    let previous = match repo.read_planning_path(path) {
        Ok(bytes) => Some(String::from_utf8(bytes)?),
        Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            None
        }
        Err(error) => return Err(error.into()),
    };
    let seed = format!(
        "<!-- koolade-artifact-id:v1 {} -->",
        serde_json::to_string(&ArtifactIdentity {
            uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            display_id: suggested_display_id.to_owned(),
            title: title.to_owned(),
            parent_uid: parent_uid.map(str::to_owned),
        })?
    );
    let trusted_previous = previous
        .as_deref()
        .filter(|previous| {
            ArtifactIdentity::from_markdown(previous)
                .ok()
                .flatten()
                .is_some()
        })
        .unwrap_or(&seed);
    ArtifactIdentity::preserve_markdown_with_parent(
        content,
        Some(trusted_previous),
        suggested_display_id,
        title,
        parent_uid,
    )
}

pub(crate) fn visible_content(markdown: &str) -> String {
    super::metadata::visible_content(markdown)
}

#[cfg(test)]
#[path = "identity/tests.rs"]
mod tests;
