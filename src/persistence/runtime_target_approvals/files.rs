use std::{fs, path::Path};

pub(super) fn validate_origin(
    workspace_id: &str,
    task_id: &str,
    origin_sha256: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        uuid::Uuid::parse_str(workspace_id).is_ok()
            && uuid::Uuid::parse_str(task_id).is_ok()
            && origin_sha256.len() == 64
            && origin_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid custom target origin binding"
    );
    Ok(())
}

#[cfg(unix)]
pub(super) fn ensure_private_dir(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "runtime target approval directory is unsafe"
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn ensure_private_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)?;
    anyhow::ensure!(
        fs::symlink_metadata(path)?.is_dir(),
        "runtime target approval directory is unsafe"
    );
    Ok(())
}

#[cfg(unix)]
pub(super) fn set_private_file(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn set_private_file(_: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn check_private_file(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    anyhow::ensure!(
        fs::metadata(path)?.permissions().mode() & 0o077 == 0,
        "runtime target approval file is not private"
    );
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn check_private_file(_: &Path) -> anyhow::Result<()> {
    Ok(())
}
