//! Plan stable identities and durable links before the schema-v3 migration writes.
mod decisions;
mod features;
mod items;
mod tasks;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use crate::domain::ArtifactIdentity;

use super::plan::Plan as ArtifactPlan;

#[derive(Clone)]
pub(super) struct Change {
    pub(super) path: String,
    pub(super) contents: String,
}

#[derive(Default)]
pub(super) struct Plan {
    features: Vec<Change>,
    tasks: Vec<Change>,
    task_uids: BTreeMap<String, String>,
    decisions: Vec<Change>,
    items: Vec<Change>,
}

impl Plan {
    pub(super) fn build(repo: &Path, migration: &ArtifactPlan) -> anyhow::Result<Self> {
        let mut seen = BTreeMap::new();
        let (features, feature_changes) = features::build(repo, migration, &mut seen)?;
        let (task_uids, task_changes) = tasks::build(repo, migration, &features, &mut seen)?;
        let decision_changes = decisions::build(repo, migration, &task_uids, &mut seen)?;
        let item_changes = items::build(repo, migration, &features, &mut seen)?;
        Ok(Self {
            features: feature_changes,
            tasks: task_changes,
            task_uids,
            decisions: decision_changes,
            items: item_changes,
        })
    }

    pub(super) fn git_paths(&self) -> Vec<String> {
        self.features
            .iter()
            .chain(&self.tasks)
            .chain(&self.decisions)
            .chain(&self.items)
            .map(|change| change.path.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.features.is_empty()
            && self.tasks.is_empty()
            && self.decisions.is_empty()
            && self.items.is_empty()
    }

    pub(super) fn task_uids(&self) -> &BTreeMap<String, String> {
        &self.task_uids
    }

    pub(super) fn apply(self, repo: &Path) -> anyhow::Result<()> {
        for changes in [self.features, self.tasks, self.decisions, self.items] {
            for change in changes {
                crate::artifacts::atomic_write(&repo.join(change.path), &change.contents)?;
            }
        }
        Ok(())
    }
}

pub(super) fn files_under(
    repo: &Path,
    migration: &ArtifactPlan,
    relative_root: &str,
) -> anyhow::Result<BTreeMap<String, String>> {
    fn walk(repo: &Path, directory: &Path, paths: &mut BTreeSet<String>) -> anyhow::Result<()> {
        let metadata = fs::symlink_metadata(directory)?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Identity migration root {} must be a real directory",
            directory.display()
        );
        let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "Identity migration refuses linked artifact {}",
                path.display()
            );
            if metadata.is_dir() {
                walk(repo, &path, paths)?;
            } else if metadata.is_file() {
                paths.insert(relative_path(repo, &path)?);
            }
        }
        Ok(())
    }

    let mut paths = BTreeSet::new();
    let root = repo.join(relative_root);
    match fs::symlink_metadata(&root) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Identity migration root {relative_root} must be a real directory"
            );
            walk(repo, &root, &mut paths)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let prefix = format!("{relative_root}/");
    paths.extend(
        migration
            .target_paths()
            .filter(|path| path.starts_with(&prefix))
            .map(str::to_owned),
    );
    let mut files = BTreeMap::new();
    for path in paths {
        if let Some(text) = read(repo, migration, &path)? {
            files.insert(path, text);
        }
    }
    Ok(files)
}

pub(super) fn read(
    repo: &Path,
    migration: &ArtifactPlan,
    relative: &str,
) -> anyhow::Result<Option<String>> {
    if let Some(bytes) = migration.planned_content(relative) {
        return Ok(Some(String::from_utf8(bytes.to_vec())?));
    }
    let path = repo.join(relative);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Identity migration artifact {relative} must be a regular file"
            );
            Ok(Some(fs::read_to_string(path)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn relative_path(repo: &Path, path: &Path) -> anyhow::Result<String> {
    path.strip_prefix(repo)?
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Artifact path is not UTF-8: {}", path.display()))
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .map(|components| components.join("/"))
}

pub(super) fn preserve_markdown(
    markdown: &str,
    suggested_display_id: &str,
    title: &str,
    parent_uid: Option<&str>,
) -> anyhow::Result<(String, ArtifactIdentity)> {
    let existing = ArtifactIdentity::from_markdown(markdown)?;
    let display_id = existing
        .as_ref()
        .map(|identity| identity.display_id.as_str())
        .unwrap_or(suggested_display_id);
    let previous = existing.as_ref().map(|_| markdown);
    let contents = ArtifactIdentity::preserve_markdown_with_parent(
        markdown, previous, display_id, title, parent_uid,
    )?;
    let identity = ArtifactIdentity::from_markdown(&contents)?
        .ok_or_else(|| anyhow::anyhow!("Packet identity was not written"))?;
    Ok((contents, identity))
}

pub(super) fn register(
    seen: &mut BTreeMap<String, String>,
    identity: &ArtifactIdentity,
    entity: &str,
) -> anyhow::Result<()> {
    if let Some(previous) = seen.insert(identity.uid.clone(), entity.to_owned()) {
        anyhow::ensure!(
            previous == entity,
            "Packet identity {} is shared by {previous} and {entity}",
            identity.uid
        );
    }
    Ok(())
}

pub(super) fn change(changes: &mut Vec<Change>, path: String, before: &str, after: String) {
    if before != after {
        changes.push(Change {
            path,
            contents: after,
        });
    }
}
