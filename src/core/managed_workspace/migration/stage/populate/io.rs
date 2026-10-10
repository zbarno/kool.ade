use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use super::{Artifact, WorkspaceManager};

pub(super) fn write_target(root: &Path, relative: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let target = safe_target(root, relative)?;
    write_exact(&target, bytes)
}

pub(super) fn write_local(
    manager: &WorkspaceManager,
    id: uuid::Uuid,
    artifact: &Artifact,
) -> anyhow::Result<()> {
    let local = artifact
        .mapping
        .target_path
        .as_deref()
        .unwrap_or_default()
        .strip_prefix("operator-local/")
        .unwrap_or_default();
    let target = safe_target(&manager.project_dir(id).join("local"), local)?;
    write_exact(&target, &artifact.bytes)
}

pub(super) fn write_gitignore(stage: &Path) -> anyhow::Result<()> {
    const CONTENT: &str = "# Operator-local secrets and runtime data\n.env\n.env.*\ncredentials/\ncredentials.*\n*.pem\n*.key\n*.p12\nimplementation/\n**/implementation/\nlogs/\n**/logs/\ntmp/\n**/tmp/\n*.log\n*.tmp\n*.temp\n*.swp\n";
    write_target(stage, ".gitignore", CONTENT.as_bytes())
}

fn safe_target(root: &Path, relative: &str) -> anyhow::Result<PathBuf> {
    let path = Path::new(relative);
    anyhow::ensure!(
        !path.as_os_str().is_empty()
            && path
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "invalid migration destination path"
    );
    if let Ok(metadata) = fs::symlink_metadata(root) {
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "migration destination root is a symlink"
        );
    }
    let target = root.join(path);
    let mut current = root.to_path_buf();
    for part in path.components() {
        current.push(part.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) => anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "migration destination contains a symlink"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(target)
}

fn write_exact(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "migration destination is a symlink: {}",
                path.display()
            );
            anyhow::ensure!(
                metadata.is_file(),
                "migration destination is not a regular file: {}",
                path.display()
            );
            anyhow::ensure!(
                fs::read(path)? == bytes,
                "migration destination collision at {}",
                path.display()
            );
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            crate::artifacts::atomic_write_bytes(path, bytes)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}
