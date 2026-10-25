use std::path::Path;

#[cfg(windows)]
mod windows_acl;

pub(super) fn verify_private_acl(path: &Path) -> anyhow::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        (metadata.is_dir() || metadata.is_file()) && !is_link_or_reparse(&metadata),
        "machine setup state path is not a regular file or directory"
    );
    #[cfg(windows)]
    windows_acl::verify_private_acl(path)?;
    Ok(())
}

pub(in crate::persistence::setup_tasks) fn ensure_private_directory(
    path: &Path,
) -> anyhow::Result<()> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        anyhow::ensure!(
            metadata.is_dir() && !is_link_or_reparse(&metadata),
            "unsafe machine setup state directory"
        );
    }
    std::fs::create_dir_all(path)?;
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !is_link_or_reparse(&metadata),
        "unsafe machine setup state directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() },
            "machine setup state directory is not owned by the current user"
        );
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        let secured = std::fs::symlink_metadata(path)?;
        anyhow::ensure!(
            secured.uid() == unsafe { libc::geteuid() }
                && secured.permissions().mode() & 0o077 == 0,
            "machine setup state directory permissions are not private"
        );
    }
    #[cfg(windows)]
    windows_acl::verify_private_acl(path)?;
    Ok(())
}

pub(super) fn secure_private_file(path: &Path) -> anyhow::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !is_link_or_reparse(&metadata),
        "machine setup state entry is not a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() },
            "machine setup state file is not owned by the current user"
        );
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        let secured = std::fs::symlink_metadata(path)?;
        anyhow::ensure!(
            secured.uid() == unsafe { libc::geteuid() }
                && secured.permissions().mode() & 0o077 == 0,
            "machine setup state file permissions are not private"
        );
    }
    #[cfg(windows)]
    windows_acl::verify_private_acl(path)?;
    Ok(())
}

pub(super) fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}
