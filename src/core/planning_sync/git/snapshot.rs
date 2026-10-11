use std::path::Path;

use super::{output, validate_path};

pub(in crate::core::planning_sync) fn read_path(
    repo: &Path,
    commit: &str,
    path: &str,
) -> anyhow::Result<Option<Vec<u8>>> {
    validate_path(path)?;
    let revision = format!("{commit}^{{commit}}");
    let resolved = output(
        repo,
        &["rev-parse", "--verify", "--end-of-options", &revision],
    )?;
    anyhow::ensure!(
        resolved.status.success(),
        "planning snapshot commit is invalid"
    );
    let commit = String::from_utf8_lossy(&resolved.stdout).trim().to_owned();
    let tree = output(
        repo,
        &["--literal-pathspecs", "ls-tree", "-z", &commit, "--", path],
    )?;
    anyhow::ensure!(
        tree.status.success(),
        "could not inspect planning snapshot tree"
    );
    let Some(entry) = tree.stdout.strip_suffix(&[0]) else {
        if tree.stdout.is_empty() {
            return Ok(None);
        }
        anyhow::bail!("planning snapshot tree entry is malformed");
    };
    if entry.is_empty() {
        return Ok(None);
    }
    let separator = entry
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| anyhow::anyhow!("planning snapshot tree entry is malformed"))?;
    let (metadata, suffix) = entry.split_at(separator);
    let entry_path = &suffix[1..];
    anyhow::ensure!(
        entry_path == path.as_bytes(),
        "planning snapshot path is ambiguous"
    );
    let fields = metadata
        .split(|byte| *byte == b' ')
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    anyhow::ensure!(
        fields.len() == 3 && fields[1] == b"blob",
        "planning snapshot path is not a file"
    );
    let object = String::from_utf8_lossy(fields[2]).to_string();
    let result = output(repo, &["cat-file", "blob", &object])?;
    anyhow::ensure!(
        result.status.success(),
        "could not read planning record blob"
    );
    Ok(Some(result.stdout))
}

pub(in crate::core::planning_sync) fn write_worktree_path(
    repo: &Path,
    path: &str,
    content: Option<&[u8]>,
) -> anyhow::Result<()> {
    use std::fs;

    validate_path(path)?;
    let relative = Path::new(path);
    let mut cursor = repo.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(part) = component else {
            anyhow::bail!("planning path is invalid");
        };
        cursor.push(part);
        let is_file = index + 1 == components.len();
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) => {
                anyhow::ensure!(
                    !metadata.file_type().is_symlink(),
                    "planning path contains a symlink"
                );
                if !is_file {
                    anyhow::ensure!(metadata.is_dir(), "planning path parent is not a directory");
                } else if content.is_some() {
                    anyhow::ensure!(
                        metadata.is_file(),
                        "planning path target is not a regular file"
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !is_file => {
                fs::create_dir(&cursor)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    match content {
        Some(bytes) => crate::artifacts::atomic_write_bytes(&cursor, bytes)?,
        None => match fs::remove_file(cursor) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        },
    }
    Ok(())
}

pub(in crate::core::planning_sync) fn changed_paths(
    repo: &Path,
    base: &str,
    target: &str,
) -> anyhow::Result<Vec<String>> {
    let result = output(
        repo,
        &["diff", "--no-renames", "--name-only", "-z", base, target],
    )?;
    anyhow::ensure!(
        result.status.success(),
        "could not compare planning snapshots"
    );
    result
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            String::from_utf8(path.to_vec())
                .map_err(|_| anyhow::anyhow!("planning snapshot contains a non-UTF-8 path"))
        })
        .collect()
}

pub(in crate::core::planning_sync) fn worktree_paths(repo: &Path) -> anyhow::Result<Vec<String>> {
    let result = output(
        repo,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    anyhow::ensure!(
        result.status.success(),
        "could not inspect planning worktree"
    );
    let mut entries = result
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty());
    let mut paths = Vec::new();
    while let Some(entry) = entries.next() {
        anyhow::ensure!(entry.len() >= 4, "planning Git status entry is malformed");
        paths.push(
            String::from_utf8(entry[3..].to_vec())
                .map_err(|_| anyhow::anyhow!("planning worktree contains a non-UTF-8 path"))?,
        );
        if entry[0] == b'R' || entry[0] == b'C' {
            let _old_path = entries.next();
        }
    }
    Ok(paths)
}
