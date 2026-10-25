use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
};

pub(super) fn create_parent_directories(root: &Path, file: &Path) -> anyhow::Result<()> {
    let relative = file.strip_prefix(root)?;
    let mut current = root.to_owned();
    let components = relative.components().collect::<Vec<_>>();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        current.push(component.as_os_str());
        if !current.exists() {
            fs::create_dir(&current)?;
            set_private_directory(&current)?;
        }
        let metadata = fs::symlink_metadata(&current)?;
        anyhow::ensure!(metadata.is_dir() && !metadata.file_type().is_symlink());
    }
    Ok(())
}

pub(super) fn open_stage_file(path: &Path, create: bool) -> anyhow::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    if create {
        options.create_new(true);
    }
    Ok(options.open(path)?)
}

pub(super) fn set_private_directory(_path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(super) fn set_readonly_private_file(_path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o400))?;
    }
    Ok(())
}

/// Make a verified private-file tree readable by the container's unprivileged
/// UID while keeping the worker-owned parent directory private.
pub(super) fn seal_mount_tree(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "private-file mount contains an unsafe directory"
    );
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            seal_mount_tree(&child)?;
        } else {
            anyhow::ensure!(
                metadata.is_file(),
                "private-file mount contains an unsafe file"
            );
            set_mode(&child, 0o444)?;
        }
    }
    set_mode(path, 0o555)
}

/// Restore owner write access before removing a previously sealed mount tree.
pub(super) fn prepare_mount_tree_for_removal(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "private-file staging root is unsafe to remove"
    );
    set_mode(path, 0o700)?;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            prepare_mount_tree_for_removal(&child)?;
        } else {
            anyhow::ensure!(
                metadata.is_file(),
                "private-file staging tree contains an unsafe file"
            );
            set_mode(&child, 0o600)?;
        }
    }
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

pub(super) fn validate_uuid(value: &str) -> anyhow::Result<()> {
    let parsed = uuid::Uuid::parse_str(value)?;
    anyhow::ensure!(parsed.hyphenated().to_string() == value);
    Ok(())
}

pub(super) fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn write_started_marker(root: &Path, execution_id: &str) -> anyhow::Result<()> {
    let marker = root.join(format!("{execution_id}.started"));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    match options.open(&marker) {
        Ok(mut file) => {
            file.write_all(b"private-file-mount-v1\n")?;
            file.sync_all()?;
            sync_directory(root)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(&marker)?;
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "private-file recovery marker is unsafe"
            );
            anyhow::ensure!(
                fs::read(&marker)? == b"private-file-mount-v1\n",
                "private-file recovery marker is invalid"
            );
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn sync_directory(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
