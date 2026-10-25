use std::{path::Path, process::Command};

use super::paths::{is_npm_package_manifest, strip_dependency_root, under_dependency_root};

pub(super) fn baseline_npm(
    worktree: &Path,
    commit: &str,
    dependency_relative_root: &str,
) -> anyhow::Result<Vec<(String, serde_json::Value)>> {
    let listing = super::git_output(
        worktree,
        &["ls-tree", "-r", "-z", "--name-only", "--full-tree", commit],
    )?;
    anyhow::ensure!(
        listing.len() <= super::MAX_TREE_LIST_BYTES,
        "Task baseline file list exceeds the npm manifest scan limit"
    );
    let mut inputs = Vec::new();
    let mut files = 0;
    let mut total_bytes = 0_usize;
    for path in listing
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(path)?;
        if !under_dependency_root(path, dependency_relative_root) || !is_npm_package_manifest(path)
        {
            continue;
        }
        files += 1;
        anyhow::ensure!(
            files <= super::MAX_LOCKFILES * 2,
            "Task baseline contains too many npm manifests"
        );
        let object = format!("{commit}:{path}");
        let size = super::git_output(worktree, &["cat-file", "-s", &object])?;
        let size = std::str::from_utf8(&size)?.trim().parse::<usize>()?;
        anyhow::ensure!(
            size <= super::MAX_BASE_MANIFEST_BYTES,
            "Task baseline npm manifest is too large"
        );
        total_bytes = total_bytes.saturating_add(size);
        anyhow::ensure!(
            total_bytes <= super::MAX_BASE_TOTAL_LOCK_BYTES,
            "Task baseline npm manifests exceed the scan budget"
        );
        let bytes = super::git_output(worktree, &["show", &object])?;
        anyhow::ensure!(
            bytes.len() == size,
            "Task baseline npm manifest changed while scanning"
        );
        let input = super::super::super::npm::dependency_manifest_inputs_from_bytes(&bytes)?;
        if !input.as_object().is_some_and(serde_json::Map::is_empty) {
            let relative_path = strip_dependency_root(path, dependency_relative_root)?;
            inputs.push((relative_path.to_owned(), input));
        }
    }
    inputs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(inputs)
}

pub(super) fn baseline_go(
    worktree: &Path,
    commit: &str,
    dependency_relative_root: &str,
) -> anyhow::Result<Vec<(String, serde_json::Value)>> {
    let manifest = manifest_path(dependency_relative_root, "go.mod");
    let sums = manifest_path(dependency_relative_root, "go.sum");
    let manifest = git_blob_if_present(worktree, commit, &manifest)?;
    let sums = git_blob_if_present(worktree, commit, &sums)?;
    match (manifest, sums) {
        (Some(manifest), Some(sums)) => {
            crate::harness::resource_bridge::go::manifest_inputs_from_bytes(&manifest, &sums)
        }
        (None, None) => Ok(Vec::new()),
        _ => anyhow::bail!("Task baseline contains an incomplete go.mod/go.sum pair"),
    }
}

fn manifest_path(dependency_relative_root: &str, file: &str) -> String {
    if dependency_relative_root == "." {
        file.to_owned()
    } else {
        format!("{dependency_relative_root}/{file}")
    }
}

fn git_blob_if_present(
    worktree: &Path,
    commit: &str,
    path: &str,
) -> anyhow::Result<Option<Vec<u8>>> {
    let object = format!("{commit}:{path}");
    let kind = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["cat-file", "-t", &object])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()?;
    if !kind.status.success() {
        return Ok(None);
    }
    anyhow::ensure!(
        std::str::from_utf8(&kind.stdout)?.trim() == "blob",
        "Task baseline Go dependency input is not a regular file"
    );
    let size = super::git_output(worktree, &["cat-file", "-s", &object])?;
    let size = std::str::from_utf8(&size)?.trim().parse::<usize>()?;
    anyhow::ensure!(
        size <= super::MAX_BASE_MANIFEST_BYTES,
        "Task baseline Go input is too large"
    );
    let bytes = super::git_output(worktree, &["show", &object])?;
    anyhow::ensure!(
        bytes.len() == size,
        "Task baseline Go input changed while scanning"
    );
    Ok(Some(bytes))
}
