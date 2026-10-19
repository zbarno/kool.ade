//! Minimal SSH-launched worker entry point. Workspace execution stays denied
//! until the verified staging flow is available.

use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

use fs2::FileExt;

use crate::execution::{
    docker::{
        backend::DockerExecutionBackend,
        engine::{DockerEndpoint, HostPlatform},
    },
    remote::staging::WorkerStager,
};

use super::{WorkerCapacityPool, server::serve_stdio_with_capacity};

const MAX_WORKER_SESSIONS: usize = super::worker_capacity::MAX_REMOTE_EXECUTIONS as usize;

/// OpenSSH starts the fixed worker command with the authenticated account's
/// effective UID. Resolve that UID through the system account database instead
/// of trusting client-supplied or inherited identity environment variables.
pub(crate) fn authenticated_ssh_principal() -> anyhow::Result<String> {
    #[cfg(target_os = "linux")]
    {
        use std::{ffi::CStr, mem::MaybeUninit};

        let suggested = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
        let buffer_size = if suggested > 0 {
            usize::try_from(suggested).unwrap_or(1024 * 1024)
        } else {
            16 * 1024
        };
        anyhow::ensure!(
            buffer_size <= 1024 * 1024,
            "worker account lookup is unavailable"
        );
        let mut buffer = vec![0_u8; buffer_size.max(1024)];
        let mut entry = MaybeUninit::<libc::passwd>::zeroed();
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(
                libc::geteuid(),
                entry.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        anyhow::ensure!(
            status == 0 && !result.is_null(),
            "worker account identity is unavailable"
        );
        let entry = unsafe { entry.assume_init() };
        anyhow::ensure!(
            !entry.pw_name.is_null(),
            "worker account identity is unavailable"
        );
        let principal = unsafe { CStr::from_ptr(entry.pw_name) }
            .to_str()?
            .to_owned();
        anyhow::ensure!(
            valid_worker_principal(&principal),
            "worker account identity is invalid"
        );
        Ok(principal)
    }
    #[cfg(not(target_os = "linux"))]
    {
        anyhow::bail!("remote worker account identity is unavailable on this platform")
    }
}

fn valid_worker_principal(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

struct WorkerSessionLease(File);

impl WorkerSessionLease {
    fn acquire(root: &Path) -> anyhow::Result<Self> {
        let directory = root.join("worker-sessions");
        fs::create_dir_all(&directory)?;
        if fs::symlink_metadata(&directory)?.file_type().is_symlink() {
            anyhow::bail!("worker session directory is not trusted");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        for slot in 0..MAX_WORKER_SESSIONS {
            let path = directory.join(format!("slot-{slot}.lock"));
            if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                anyhow::bail!("worker session lock is not trusted");
            }
            let mut options = OpenOptions::new();
            options.read(true).write(true).create(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
            }
            let file = options.open(path)?;
            match FileExt::try_lock_exclusive(&file) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        file.set_permissions(fs::Permissions::from_mode(0o600))?;
                    }
                    return Ok(Self(file));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(error) => return Err(error.into()),
            }
        }
        anyhow::bail!("remote worker session limit is reached")
    }
}

impl Drop for WorkerSessionLease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

/// Run the fixed stdio worker command used by the SSH controller transport.
/// Standard output contains protocol frames only; startup diagnostics use stderr.
pub fn serve_stdio_worker() -> anyhow::Result<()> {
    anyhow::ensure!(
        cfg!(target_os = "linux"),
        "remote workers currently require Linux"
    );
    let endpoint = DockerEndpoint::from_process_environment(HostPlatform::Linux)
        .map_err(|failure| anyhow::anyhow!(failure.message))?;
    let state_root = crate::persistence::state_root();
    let _session = WorkerSessionLease::acquire(&state_root)?;
    let worker_id = crate::persistence::execution_records::owner_instance_id()
        .map_err(|_| anyhow::anyhow!("worker identity is unavailable"))?;
    let capacity = std::sync::Arc::new(WorkerCapacityPool::new(&state_root, worker_id.clone())?);
    let stager = std::sync::Arc::new(WorkerStager::new(&state_root, worker_id.clone())?);
    let resolver = stager.clone();
    let backend = DockerExecutionBackend::connect(endpoint, resolver, worker_id)?;
    serve_stdio_with_capacity(
        std::sync::Arc::new(backend),
        Some(stager),
        Some(capacity),
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_sessions_are_bounded_and_released_on_drop() {
        let root = tempfile::tempdir().unwrap();
        let mut sessions = (0..MAX_WORKER_SESSIONS)
            .map(|_| WorkerSessionLease::acquire(root.path()).unwrap())
            .collect::<Vec<_>>();
        assert!(WorkerSessionLease::acquire(root.path()).is_err());
        sessions.pop();
        assert!(WorkerSessionLease::acquire(root.path()).is_ok());
    }
}
