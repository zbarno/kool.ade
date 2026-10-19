use super::fs_helpers::prepare_mount_tree_for_removal;
use super::{PrivateFiles, validate_uuid};
use std::fs;

impl PrivateFiles {
    pub(in crate::execution::remote::staging::worker) fn has_stages(&self) -> bool {
        self.executions
            .lock()
            .map_or(true, |executions| !executions.is_empty())
    }

    pub(in crate::execution::remote::staging::worker) fn cleanup_recovered(
        &self,
    ) -> anyhow::Result<()> {
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(execution_id) = name.to_str() else {
                continue;
            };
            let Ok(parsed) = uuid::Uuid::parse_str(execution_id) else {
                continue;
            };
            if parsed.hyphenated().to_string() != execution_id {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "private-file staging directory is unsafe"
            );
            let _owner_lock = match self.try_owner_lock(execution_id) {
                Ok(lock) => lock,
                Err(error) if is_lock_busy(&error) => continue,
                Err(error) => return Err(error),
            };
            // This sweep runs after Docker recovery. Stages without a Start
            // marker could not have been mounted into a container.
            remove_stage_directory(&entry.path())?;
            remove_recovery_marker(&self.root.join(format!("{execution_id}.started")))?;
        }

        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(execution_id) = name.strip_suffix(".started") else {
                continue;
            };
            validate_uuid(execution_id)?;
            if self.execution_root(execution_id)?.exists() {
                continue;
            }
            validate_recovery_marker(&entry.path())?;
            let _owner_lock = match self.try_owner_lock(execution_id) {
                Ok(lock) => lock,
                Err(error) if is_lock_busy(&error) => continue,
                Err(error) => return Err(error),
            };
            remove_recovery_marker(&entry.path())?;
        }
        Ok(())
    }

    pub(in crate::execution::remote::staging::worker) fn cleanup(
        &self,
        execution_id: &str,
        after_stop: bool,
    ) -> anyhow::Result<()> {
        let mut executions = self
            .executions
            .lock()
            .map_err(|_| anyhow::anyhow!("private-file grant state is unavailable"))?;
        let Some(execution) = executions.get(execution_id) else {
            if !after_stop {
                return Ok(());
            }
            let _owner_lock = self.try_owner_lock(execution_id)?;
            remove_stage_directory(&self.execution_root(execution_id)?)?;
            remove_recovery_marker(&self.root.join(format!("{execution_id}.started")))?;
            return Ok(());
        };
        if execution.mounted && !after_stop {
            anyhow::bail!("active private-file mount cannot be discarded before stop");
        }
        remove_stage_directory(&self.execution_root(execution_id)?)?;
        remove_recovery_marker(&self.root.join(format!("{execution_id}.started")))?;
        executions.remove(execution_id);
        Ok(())
    }
}

fn is_lock_busy(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
}

fn validate_recovery_marker(marker: &std::path::Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(marker)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "private-file recovery marker is unsafe"
    );
    anyhow::ensure!(
        fs::read(marker)? == b"private-file-mount-v1\n",
        "private-file recovery marker is invalid"
    );
    Ok(())
}

fn remove_stage_directory(directory: &std::path::Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "private-file staging root is unsafe to remove"
            );
            prepare_mount_tree_for_removal(directory)?;
            fs::remove_dir_all(directory)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn remove_recovery_marker(marker: &std::path::Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(marker) {
        Ok(_) => {
            validate_recovery_marker(marker)?;
            fs::remove_file(marker)?;
            fs::File::open(marker.parent().expect("marker has parent"))?.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
