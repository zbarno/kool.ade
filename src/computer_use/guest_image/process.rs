mod runner;

#[cfg(all(test, unix))]
#[path = "process/tests.rs"]
mod tests;

use std::{ffi::OsStr, path::PathBuf, process::ExitStatus, time::Duration};

#[cfg(target_os = "linux")]
use std::path::Path;

use self::runner::run_bounded;
#[cfg(target_os = "linux")]
use self::runner::run_bounded_with_secret_fd;

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

#[cfg(target_os = "linux")]
pub(super) fn run_qemu_with_secret_fd(
    tool: &QemuImageToolV1,
    args: &[&OsStr],
    secret_fd: libc::c_int,
    max_file_bytes: u64,
) -> Result<BoundedOutput, QemuProcessError> {
    run_bounded_with_secret_fd(&tool.path, args, QEMU_TIMEOUT, secret_fd, max_file_bytes)
}

pub fn probe_host_qemu_image_tool() -> Result<QemuImageToolV1, QemuProcessError> {
    probe_host_qemu_image_tool_with_timeout(QEMU_TIMEOUT)
}

pub fn probe_host_qemu_image_tool_for_readiness() -> Result<(), QemuProcessError> {
    probe_host_qemu_image_tool_with_timeout(Duration::from_secs(2)).map(|_| ())
}

fn probe_host_qemu_image_tool_with_timeout(
    timeout: Duration,
) -> Result<QemuImageToolV1, QemuProcessError> {
    #[cfg(target_os = "linux")]
    {
        let path = PathBuf::from("/usr/bin/qemu-img");
        validate_root_owned_executable(&path)?;
        let output = run_bounded(&path, &[OsStr::new("--version")], timeout)?;
        if !output.status.success() || !qemu_img_version_is_valid(&output.stdout) {
            return Err(QemuProcessError::Unavailable);
        }
        Ok(QemuImageToolV1 { path })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = timeout;
        Err(QemuProcessError::Unavailable)
    }
}

#[cfg(target_os = "linux")]
fn qemu_img_version_is_valid(output: &[u8]) -> bool {
    let Ok(output) = std::str::from_utf8(output) else {
        return false;
    };
    let Some(line) = output.lines().next() else {
        return false;
    };
    if line.len() > 160 || line.chars().any(char::is_control) {
        return false;
    }
    let Some(version) = line.strip_prefix("qemu-img version ") else {
        return false;
    };
    let version = version.split_ascii_whitespace().next().unwrap_or_default();
    let mut parts = version.split('.');
    let numeric = [parts.next(), parts.next(), parts.next()];
    numeric.into_iter().all(|part| {
        part.is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) && parts.next().is_none()
}

#[cfg(target_os = "linux")]
fn validate_root_owned_executable(path: &Path) -> Result<(), QemuProcessError> {
    crate::computer_use::validate_trusted_system_executable(path)
        .map_err(|_| QemuProcessError::Unavailable)
}

#[cfg(test)]
impl QemuImageToolV1 {
    pub(super) fn from_test_path(path: PathBuf) -> Self {
        Self { path }
    }
}
