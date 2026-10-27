use std::{
    io::Read,
    path::{Path, PathBuf},
};

use serde_json::Value;

#[cfg(windows)]
const MAX_COPIED_ASSET_BYTES: u64 = 512 * 1024 * 1024;
#[cfg(windows)]
const MAX_COPIED_ASSET_ENTRIES: usize = 50_000;
#[cfg(windows)]
const MAX_COPIED_ASSET_DEPTH: usize = 64;

/// Bridge only the built-in assets from the Pi package owning the executable.
/// The clean profile still excludes user configuration, auth, extensions and sessions.
pub(super) fn link_builtin_package_assets(
    package_directory: &Path,
    pi_executable: &Path,
) -> anyhow::Result<()> {
    let root = find_pi_package_root(pi_executable).ok_or_else(|| {
        anyhow::anyhow!(
            "Cannot locate the Pi package owning {}; the isolated Pi session needs its built-in assets. \
             Ensure pi was installed from its npm package layout, or point the pinned binary at the packaged executable.",
            pi_executable.display()
        )
    })?;
    let metadata_path = root.join("package.json");
    let metadata_file = std::fs::symlink_metadata(&metadata_path)?;
    anyhow::ensure!(
        metadata_file.is_file()
            && !metadata_file.file_type().is_symlink()
            && metadata_file.len() <= 256 * 1024,
        "Pi package metadata in {} is not a bounded regular file",
        root.display()
    );
    let mut raw_metadata = Vec::with_capacity(metadata_file.len() as usize);
    std::fs::File::open(&metadata_path)?
        .take(256 * 1024 + 1)
        .read_to_end(&mut raw_metadata)?;
    anyhow::ensure!(
        raw_metadata.len() <= 256 * 1024,
        "Pi package metadata in {} exceeds the size bound",
        root.display()
    );
    let metadata: Value = serde_json::from_slice(&raw_metadata).map_err(|error| {
        anyhow::anyhow!(
            "Pi package metadata in {} is not valid JSON ({error})",
            root.display()
        )
    })?;
    anyhow::ensure!(
        metadata.get("name").and_then(Value::as_str) == Some("@earendil-works/pi-coding-agent"),
        "The Pi package in {} failed the identity check (unexpected package name)",
        root.display()
    );
    for entry in ["dist", "docs", "package.json"] {
        let from = root.join(entry);
        let source_metadata = match std::fs::symlink_metadata(&from) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && entry == "docs" => {
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                anyhow::bail!("Required Pi package asset {} is missing", from.display())
            }
            Err(error) => anyhow::bail!(
                "Cannot inspect {} ({error}); the Pi package appears damaged",
                from.display()
            ),
        };
        let expected_type = if entry == "package.json" {
            source_metadata.is_file()
        } else {
            source_metadata.is_dir()
        };
        anyhow::ensure!(
            expected_type && !source_metadata.file_type().is_symlink(),
            "Pi package asset {} has an unexpected or symbolic-link file type",
            from.display()
        );
        let to = package_directory.join(entry);
        if to.symlink_metadata().is_ok() {
            continue;
        }
        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&from, &to);
        #[cfg(windows)]
        let link_result = if source_metadata.is_dir() {
            std::os::windows::fs::symlink_dir(&from, &to)
        } else {
            std::os::windows::fs::symlink_file(&from, &to)
        };
        if let Err(link_error) = link_result {
            if source_metadata.is_file() {
                std::fs::copy(&from, &to)?;
            } else {
                #[cfg(windows)]
                {
                    let mut budget = CopyBudget::default();
                    copy_asset_tree(&from, &to, 0, &mut budget).map_err(|copy_error| {
                        anyhow::anyhow!(
                            "{entry}: cannot link ({link_error}) or safely copy built-in assets ({copy_error})"
                        )
                    })?;
                }
                #[cfg(not(windows))]
                anyhow::bail!("{entry}: cannot bridge built-in assets ({link_error})");
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
#[derive(Default)]
struct CopyBudget {
    bytes: u64,
    entries: usize,
}

#[cfg(windows)]
fn copy_asset_tree(
    source: &Path,
    destination: &Path,
    depth: usize,
    budget: &mut CopyBudget,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        depth <= MAX_COPIED_ASSET_DEPTH,
        "Pi package asset tree is too deep"
    );
    std::fs::create_dir(destination)?;
    for item in std::fs::read_dir(source)? {
        let item = item?;
        budget.entries += 1;
        anyhow::ensure!(
            budget.entries <= MAX_COPIED_ASSET_ENTRIES,
            "Pi package asset tree contains too many entries"
        );
        let metadata = std::fs::symlink_metadata(item.path())?;
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "refusing symbolic link {} in Pi package assets",
            item.path().display()
        );
        let target = destination.join(item.file_name());
        if metadata.is_dir() {
            copy_asset_tree(&item.path(), &target, depth + 1, budget)?;
        } else {
            anyhow::ensure!(
                metadata.is_file(),
                "refusing non-file asset {}",
                item.path().display()
            );
            budget.bytes = budget.bytes.saturating_add(metadata.len());
            anyhow::ensure!(
                budget.bytes <= MAX_COPIED_ASSET_BYTES,
                "Pi package assets exceed the copy size limit"
            );
            std::fs::copy(item.path(), target)?;
        }
    }
    Ok(())
}

/// Mirrors Pi's package-root walk: the nearest `dist` ancestor whose parent
/// has package metadata marks the owning package.
fn find_pi_package_root(pi_executable: &Path) -> Option<PathBuf> {
    let mut dir = pi_executable.parent()?.to_path_buf();
    for _ in 0..8 {
        if dir.ends_with("dist")
            && dir
                .parent()
                .is_some_and(|parent| parent.join("package.json").exists())
        {
            return dir.parent().map(Path::to_path_buf);
        }
        let Some(parent) = dir.parent() else {
            break;
        };
        dir = parent.to_path_buf();
    }
    None
}
