use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
};

use super::super::{git, journal::OperationJournal, model::PlanningSyncStatus};
use super::intent::shared_manifest;

pub(crate) fn export_pending_at(
    repo: &Path,
    state_root: &Path,
    destination: &Path,
) -> anyhow::Result<PlanningSyncStatus> {
    let (manifest, _) = shared_manifest(repo)?
        .ok_or_else(|| anyhow::anyhow!("planning repository is not managed"))?;
    let journal =
        OperationJournal::load_or_new(state_root, manifest.workspace_id, &manifest.canonical_ref)?;
    let unconfirmed_snapshot =
        if journal.confirmed_commit.is_none() && journal.operations.is_empty() {
            Some(exportable_snapshot(repo)?)
        } else {
            None
        };
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "schemaVersion": 1,
        "workspaceId": manifest.workspace_id,
        "canonicalRef": manifest.canonical_ref,
        "operations": &journal.operations,
        "conflicts": &journal.conflicts,
        "resolutions": &journal.resolutions,
        "unconfirmedSnapshot": unconfirmed_snapshot,
    }))?;
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow::anyhow!("export path must have a parent directory"))?;
    anyhow::ensure!(parent.is_dir(), "export directory does not exist");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| anyhow::anyhow!("export file could not be created: {error}"))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(journal.status())
}

fn exportable_snapshot(repo: &Path) -> anyhow::Result<Vec<serde_json::Value>> {
    let result = git::output(
        repo,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?;
    anyhow::ensure!(
        result.status.success(),
        "planning snapshot could not be listed"
    );
    let root = repo.canonicalize()?;
    let paths = result
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            String::from_utf8(path.to_vec())
                .map_err(|_| anyhow::anyhow!("planning snapshot contains a non-UTF-8 path"))
        })
        .collect::<anyhow::Result<BTreeSet<_>>>()?;
    let mut snapshot = Vec::new();
    for path in paths.into_iter().filter(|path| exportable_path(path)) {
        let content = read_snapshot_file(&root, &path)?;
        snapshot.push(serde_json::json!({"path": path, "content": content}));
    }
    Ok(snapshot)
}

fn exportable_path(path: &str) -> bool {
    matches!(
        path,
        "manifest.json" | "config/project.md" | "config/repositories.json"
    ) || path.starts_with("planning/")
        || path.starts_with("state/")
}

fn read_snapshot_file(root: &Path, path: &str) -> anyhow::Result<Option<String>> {
    let relative = Path::new(path);
    anyhow::ensure!(
        !path.starts_with('/')
            && !path.contains('\\')
            && relative
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "planning snapshot contains an invalid path"
    );
    let mut current = PathBuf::from(root);
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            anyhow::bail!("planning snapshot contains an invalid path")
        };
        current.push(name);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "planning snapshot contains a symbolic link"
        );
        let is_file = index + 1 == components.len();
        anyhow::ensure!(
            if is_file {
                metadata.is_file()
            } else {
                metadata.is_dir()
            },
            "planning snapshot contains an invalid file path"
        );
    }
    let bytes = fs::read(current)?;
    Ok(Some(String::from_utf8(bytes).map_err(|_| {
        anyhow::anyhow!("planning snapshot contains a non-UTF-8 file")
    })?))
}
