mod runner;

#[cfg(all(test, unix))]
#[path = "process/tests.rs"]
mod tests;

use std::{ffi::OsStr, path::PathBuf, process::ExitStatus, time::Duration};

#[cfg(target_os = "linux")]
use std::path::Path;

use self::runner::run_bounded;

#[cfg(unix)]
const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const QEMU_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug)]
pub enum QemuProcessError {
    Unavailable,
    Failed,
    TimedOut,
    OutputTooLarge,
}

pub struct QemuImageToolV1 {
    path: PathBuf,
}

pub(super) struct BoundedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
}

pub(super) fn run_qemu(
    tool: &QemuImageToolV1,
    args: &[&OsStr],
) -> Result<BoundedOutput, QemuProcessError> {
    run_bounded(&tool.path, args, QEMU_TIMEOUT)
}

pub fn probe_host_qemu_image_tool() -> Result<QemuImageToolV1, QemuProcessError> {
    #[cfg(target_os = "linux")]
    {
        let path = PathBuf::from("/usr/bin/qemu-img");
        validate_root_owned_executable(&path)?;
        let output = run_bounded(&path, &[OsStr::new("--version")], QEMU_TIMEOUT)?;
        let version = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() || !version.starts_with("qemu-img version ") {
            return Err(QemuProcessError::Unavailable);
        }
        Ok(QemuImageToolV1 { path })
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(QemuProcessError::Unavailable)
    }
}

#[cfg(target_os = "linux")]
fn validate_root_owned_executable(path: &Path) -> Result<(), QemuProcessError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let canonical = path
        .canonicalize()
        .map_err(|_| QemuProcessError::Unavailable)?;
    if canonical != path {
        return Err(QemuProcessError::Unavailable);
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| QemuProcessError::Unavailable)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != 0
        || metadata.permissions().mode() & 0o022 != 0
        || metadata.permissions().mode() & 0o111 == 0
    {
        return Err(QemuProcessError::Unavailable);
    }
    for ancestor in [Path::new("/"), Path::new("/usr"), Path::new("/usr/bin")] {
        let metadata =
            std::fs::symlink_metadata(ancestor).map_err(|_| QemuProcessError::Unavailable)?;
        if !metadata.file_type().is_dir()
            || metadata.uid() != 0
            || metadata.permissions().mode() & 0o022 != 0
        {
            return Err(QemuProcessError::Unavailable);
        }
    }
    Ok(())
}

#[cfg(test)]
impl QemuImageToolV1 {
    pub(super) fn from_test_path(path: PathBuf) -> Self {
        Self { path }
    }
}
