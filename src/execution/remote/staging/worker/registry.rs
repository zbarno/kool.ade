use super::*;
use crate::execution::remote::WorkspaceManifestV1;
use std::fs::OpenOptions;

impl WorkerStager {
    fn registry_path(&self, workspace: &WorkspaceRef, extension: &str) -> anyhow::Result<PathBuf> {
        workspace.validate().map_err(anyhow::Error::msg)?;
        let root = self.staging_root.join("workspace-registry");
        create_private_dir(&root)?;
        Ok(root.join(format!(
            "{}-{}-{}.{}",
            workspace.project_id, workspace.task_id, workspace.source_sha, extension
        )))
    }

    /// Separate descriptors share the same OS lock across SSH worker processes.
    pub(super) fn lock_registry(
        &self,
        workspace: &WorkspaceRef,
        exclusive: bool,
    ) -> anyhow::Result<fs::File> {
        let path = self.registry_path(workspace, "lock")?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(path)?;
        if exclusive {
            fs2::FileExt::try_lock_exclusive(&file)?;
        } else {
            fs2::FileExt::try_lock_shared(&file)?;
        }
        Ok(file)
    }

    pub(super) fn selected_workspace(
        &self,
        workspace: &WorkspaceRef,
    ) -> anyhow::Result<Option<WorkspaceManifestV1>> {
        let path = self.registry_path(workspace, "json")?;
        if !path.try_exists()? {
            return Ok(None);
        }
        let snapshot: WorkspaceManifestV1 =
            serde_json::from_slice(&read_bounded(&path, 512 * 1024)?)?;
        snapshot.validate()?;
        anyhow::ensure!(
            snapshot.workspace == *workspace,
            "selected workspace identity differs"
        );
        Ok(Some(snapshot))
    }

    pub(super) fn select_workspace(&self, snapshot: &WorkspaceManifestV1) -> anyhow::Result<()> {
        let path = self.registry_path(&snapshot.workspace, "json")?;
        crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec(snapshot)?)?;
        set_private_file(&path)
    }

    /// A crashed worker drops its locks, but its durable execution may still own
    /// the current generation. Keep that generation until cleanup is complete.
    pub(super) fn ensure_no_unfinished_execution(
        &self,
        workspace: &WorkspaceRef,
    ) -> anyhow::Result<()> {
        use crate::persistence::execution_records::{ExecutionCleanupStateV1, ExecutionRecordV1};
        let root = self
            .staging_root
            .parent()
            .expect("staging root has parent")
            .join("executions")
            .join(&workspace.task_id);
        if !root.try_exists()? {
            return Ok(());
        }
        let metadata = fs::symlink_metadata(&root)?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "execution record root is unsafe"
        );
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let record: ExecutionRecordV1 =
                serde_json::from_slice(&read_bounded(&path, 1024 * 1024)?)?;
            record.validate().map_err(anyhow::Error::msg)?;
            anyhow::ensure!(
                record.task_id == workspace.task_id,
                "execution task identity differs"
            );
            anyhow::ensure!(
                record.cleanup == ExecutionCleanupStateV1::Complete,
                "workspace generation has an unfinished durable execution"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
