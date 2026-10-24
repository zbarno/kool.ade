use std::path::{Component, Path};

pub(super) fn ensure_private_directory_chain(root: &Path, directory: &Path) -> Result<(), String> {
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| "runtime QA record path escaped its private store".to_owned())?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("cannot create private runtime QA directory: {error}"))?;
    secure_directory(root)?;

    let mut current = root.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(name) => current.push(name),
            _ => return Err("runtime QA record path contains an invalid directory".into()),
        }
        secure_directory(&current)?;
    }
    Ok(())
}

pub(super) fn secure_private_file(root: &Path, path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "runtime QA record path has no parent directory".to_owned())?;
    ensure_private_directory_chain(root, parent)?;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect private runtime QA file: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("runtime QA private file is not a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err("runtime QA private file is not owned by the current user".into());
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("cannot secure private runtime QA file: {error}"))?;
        let secured = std::fs::symlink_metadata(path)
            .map_err(|error| format!("cannot verify private runtime QA file: {error}"))?;
        if !secured.is_file()
            || secured.file_type().is_symlink()
            || secured.uid() != unsafe { libc::geteuid() }
            || secured.permissions().mode() & 0o077 != 0
        {
            return Err("runtime QA file permissions are not private to the current user".into());
        }
    }
    Ok(())
}

fn secure_directory(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect private runtime QA directory: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("runtime QA private path is not a directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err("runtime QA private directory is not owned by the current user".into());
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("cannot secure private runtime QA directory: {error}"))?;
        let secured = std::fs::symlink_metadata(path)
            .map_err(|error| format!("cannot verify private runtime QA directory: {error}"))?;
        if !secured.is_dir()
            || secured.file_type().is_symlink()
            || secured.uid() != unsafe { libc::geteuid() }
            || secured.permissions().mode() & 0o077 != 0
        {
            return Err(
                "runtime QA directory permissions are not private to the current user".into(),
            );
        }
    }
    Ok(())
}
