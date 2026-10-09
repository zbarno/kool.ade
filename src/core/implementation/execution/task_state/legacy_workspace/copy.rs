use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_SNAPSHOT_FILES: usize = 20_000;
const MAX_SNAPSHOT_BYTES: u64 = 128 * 1024 * 1024;

pub(super) fn paths_from_nul_list(data: &[u8]) -> anyhow::Result<Vec<PathBuf>> {
    data.split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            #[cfg(unix)]
            let path = {
                use std::os::unix::ffi::OsStringExt;
                PathBuf::from(std::ffi::OsString::from_vec(path.to_vec()))
            };
            #[cfg(not(unix))]
            let path = PathBuf::from(std::str::from_utf8(path)?);
            validate_relative(&path)?;
            Ok(path)
        })
        .collect()
}

pub(super) fn copy_untracked(
    source_root: &Path,
    destination_root: &Path,
    list: &Path,
    replace_changed: bool,
) -> anyhow::Result<()> {
    let paths = paths_from_nul_list(&fs::read(list)?)?;
    for relative in paths {
        let source = source_root.join(&relative);
        let destination = destination_root.join(&relative);
        validate_source_path(source_root, &relative)?;
        ensure_parent_dirs(destination_root, destination.parent().unwrap(), true)?;
        copy_node(&source, &destination, replace_changed)?;
    }
    Ok(())
}

pub(super) fn ensure_snapshot_budget(source_root: &Path, list: &Path) -> anyhow::Result<()> {
    let paths = paths_from_nul_list(&fs::read(list)?)?;
    anyhow::ensure!(
        paths.len() <= MAX_SNAPSHOT_FILES,
        "Ignored files exceed the migration file-count limit; the original workspace is preserved"
    );
    let mut total_bytes = 0_u64;
    for relative in paths {
        validate_source_path(source_root, &relative)?;
        total_bytes = total_bytes
            .checked_add(fs::symlink_metadata(source_root.join(relative))?.len())
            .ok_or_else(|| anyhow::anyhow!("Ignored-file migration size overflow"))?;
        anyhow::ensure!(
            total_bytes <= MAX_SNAPSHOT_BYTES,
            "Ignored files exceed the 128 MiB migration limit; the original workspace is preserved"
        );
    }
    Ok(())
}

pub(super) fn untracked_match(
    source_root: &Path,
    snapshot_root: &Path,
    list: &Path,
) -> anyhow::Result<bool> {
    for relative in paths_from_nul_list(&fs::read(list)?)? {
        validate_source_path(source_root, &relative)?;
        if !same_node(&source_root.join(&relative), &snapshot_root.join(&relative))? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn validate_source_path(root: &Path, relative: &Path) -> anyhow::Result<()> {
    validate_relative(relative)?;
    let mut current = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        let Component::Normal(name) = component else {
            anyhow::bail!("Untracked path is not repository-relative");
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Untracked path crosses a symlinked parent; the original is preserved for review"
        );
    }
    let metadata = fs::symlink_metadata(root.join(relative))?;
    anyhow::ensure!(
        metadata.is_file() || metadata.file_type().is_symlink(),
        "Unsupported untracked filesystem object; the original is preserved for review"
    );
    Ok(())
}

fn validate_relative(path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty()
            && !path.is_absolute()
            && path
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "Untracked path is not a normalized repository-relative path"
    );
    Ok(())
}

fn ensure_parent_dirs(root: &Path, parent: &Path, create: bool) -> anyhow::Result<()> {
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| anyhow::anyhow!("Untracked destination escaped its snapshot root"))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            anyhow::bail!("Untracked destination path is invalid");
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Untracked destination parent is not a real directory"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                fs::create_dir(&current)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn copy_node(source: &Path, destination: &Path, replace_changed: bool) -> anyhow::Result<()> {
    if same_node(source, destination)? {
        return Ok(());
    }
    match fs::symlink_metadata(destination) {
        Ok(_) if !replace_changed => {
            anyhow::bail!("A migrated untracked path changed; both copies are preserved")
        }
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            anyhow::bail!("A migrated untracked path collides with a directory")
        }
        Ok(_) => fs::remove_file(destination)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source)?;
        create_symlink(&target, destination)?;
    } else {
        fs::copy(source, destination)?;
        fs::set_permissions(destination, metadata.permissions())?;
    }
    Ok(())
}

fn same_node(left: &Path, right: &Path) -> anyhow::Result<bool> {
    let left_meta = match fs::symlink_metadata(left) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let right_meta = match fs::symlink_metadata(right) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if left_meta.file_type().is_symlink() || right_meta.file_type().is_symlink() {
        return Ok(left_meta.file_type().is_symlink()
            && right_meta.file_type().is_symlink()
            && fs::read_link(left)? == fs::read_link(right)?);
    }
    if !left_meta.is_file() || !right_meta.is_file() || left_meta.len() != right_meta.len() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if left_meta.permissions().mode() & 0o777 != right_meta.permissions().mode() & 0o777 {
            return Ok(false);
        }
    }
    let mut left = fs::File::open(left)?;
    let mut right = fs::File::open(right)?;
    let mut left_buf = [0_u8; 16 * 1024];
    let mut right_buf = [0_u8; 16 * 1024];
    loop {
        let left_read = left.read(&mut left_buf)?;
        let right_read = right.read(&mut right_buf)?;
        if left_read != right_read || left_buf[..left_read] != right_buf[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

#[cfg(unix)]
fn create_symlink(target: &Path, destination: &Path) -> anyhow::Result<()> {
    std::os::unix::fs::symlink(target, destination)?;
    Ok(())
}

#[cfg(windows)]
fn create_symlink(target: &Path, destination: &Path) -> anyhow::Result<()> {
    std::os::windows::fs::symlink_file(target, destination)?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn create_symlink(_: &Path, _: &Path) -> anyhow::Result<()> {
    anyhow::bail!("Symlink migration is unsupported on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nul_path_list_preserves_tabs_newlines_and_non_utf8_names() {
        let paths = paths_from_nul_list(b"tab\tname\0line\nname\0binary\xff\0").unwrap();
        assert_eq!(paths.len(), 3);
        assert_eq!(paths[0], Path::new("tab\tname"));
        assert_eq!(paths[1], Path::new("line\nname"));
    }
}
