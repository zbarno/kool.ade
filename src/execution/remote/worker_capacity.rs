//! Worker-host slot leases shared by every authenticated SSH controller.

use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    path::PathBuf,
};

pub(crate) const MAX_REMOTE_EXECUTIONS: u32 = 8;

pub(crate) struct WorkerCapacityPool {
    directory: PathBuf,
    max_jobs: u32,
}

pub(crate) struct WorkerCapacityLease(File);

impl WorkerCapacityPool {
    pub(crate) fn new(root: &std::path::Path, worker_id: String) -> anyhow::Result<Self> {
        crate::execution::contract::validate_identifier(&worker_id)?;
        let directory = root.join("worker-capacity").join(&worker_id);
        fs::create_dir_all(&directory)?;
        let metadata = fs::symlink_metadata(&directory)?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "worker capacity directory is not trusted"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            directory,
            max_jobs: MAX_REMOTE_EXECUTIONS,
        })
    }

    pub(crate) fn max_jobs(&self) -> u32 {
        self.max_jobs
    }

    pub(crate) fn acquire(&self) -> anyhow::Result<WorkerCapacityLease> {
        for slot in 0..self.max_jobs {
            let path = self.directory.join(format!("slot-{slot}.lock"));
            if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                anyhow::bail!("worker capacity slot is not trusted");
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
                Ok(()) => return Ok(WorkerCapacityLease(file)),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(error) => return Err(error.into()),
            }
        }
        anyhow::bail!("remote worker execution capacity is full")
    }
}

impl Drop for WorkerCapacityLease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

#[cfg(test)]
#[path = "worker_capacity/tests.rs"]
mod tests;
