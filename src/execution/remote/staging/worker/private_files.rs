use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::execution::remote::private_files::{PrivateFileChunkV1, RemotePrivateFileGrantV1};
use zeroize::Zeroizing;

use super::create_private_dir;
mod fs_helpers;
mod lifecycle;
#[cfg(test)]
mod tests;
use fs_helpers::{
    create_parent_directories, digest, open_stage_file, seal_mount_tree, set_private_directory,
    set_readonly_private_file, validate_uuid, write_started_marker,
};

#[derive(Default)]
struct ExecutionFiles {
    grants: HashMap<String, RemotePrivateFileGrantV1>,
    files: HashMap<String, StagedFile>,
    mounted: bool,
    _owner_lock: Option<File>,
}

struct StagedFile {
    grant_id: String,
    spec: crate::execution::remote::private_files::PrivateFileSpecV1,
    next_offset: u64,
    complete: bool,
}

pub(super) struct PrivateFiles {
    root: PathBuf,
    executions: Mutex<HashMap<String, ExecutionFiles>>,
}

impl PrivateFiles {
    pub(super) fn new(staging_root: &Path) -> anyhow::Result<Self> {
        let root = staging_root.join("private-files");
        create_private_dir(&root)?;
        Ok(Self {
            root,
            executions: Mutex::new(HashMap::new()),
        })
    }

    pub(super) fn authorize(
        &self,
        execution_id: &str,
        worker_id: &str,
        grant: RemotePrivateFileGrantV1,
        workspace_is_staged: bool,
        now_unix_ms: u64,
    ) -> anyhow::Result<()> {
        validate_uuid(execution_id)?;
        anyhow::ensure!(workspace_is_staged, "private-file workspace is not staged");
        grant.validate_at(now_unix_ms)?;
        anyhow::ensure!(grant.worker_id == worker_id, "grant targets another worker");
        let mut executions = self
            .executions
            .lock()
            .map_err(|_| anyhow::anyhow!("private-file grant state is unavailable"))?;
        if !executions.contains_key(execution_id) {
            let owner_lock = self.try_owner_lock(execution_id)?;
            let directory = self.execution_root(execution_id)?;
            fs::create_dir(&directory)?;
            set_private_directory(&directory)?;
            executions.insert(
                execution_id.to_owned(),
                ExecutionFiles {
                    _owner_lock: Some(owner_lock),
                    ..Default::default()
                },
            );
        }
        let execution = executions.get_mut(execution_id).expect("inserted above");
        if let Some(existing) = execution.grants.get(&grant.grant_id) {
            anyhow::ensure!(existing == &grant, "private-file grant ID conflicts");
            return Ok(());
        }
        for spec in &grant.files {
            anyhow::ensure!(
                !execution.files.contains_key(&spec.path),
                "multiple private-file grants select the same path"
            );
        }
        for spec in &grant.files {
            let empty = spec.size_bytes == 0;
            if empty {
                let path = self.file_path(execution_id, &spec.path)?;
                create_parent_directories(&self.execution_root(execution_id)?, &path)?;
                let file = open_stage_file(&path, true)?;
                file.sync_all()?;
                drop(file);
                anyhow::ensure!(
                    spec.sha256 == digest(&[]),
                    "empty private-file digest does not match its grant"
                );
                set_readonly_private_file(&path)?;
            }
            execution.files.insert(
                spec.path.clone(),
                StagedFile {
                    grant_id: grant.grant_id.clone(),
                    spec: spec.clone(),
                    next_offset: 0,
                    complete: empty,
                },
            );
        }
        execution.grants.insert(grant.grant_id.clone(), grant);
        Ok(())
    }

    pub(super) fn receive(
        &self,
        execution_id: &str,
        chunk: &PrivateFileChunkV1,
        now_unix_ms: u64,
    ) -> anyhow::Result<()> {
        let mut executions = self
            .executions
            .lock()
            .map_err(|_| anyhow::anyhow!("private-file grant state is unavailable"))?;
        let execution = executions
            .get_mut(execution_id)
            .ok_or_else(|| anyhow::anyhow!("private-file execution grant is unavailable"))?;
        let grant = execution
            .grants
            .get(&chunk.grant_id)
            .ok_or_else(|| anyhow::anyhow!("private-file grant is unavailable"))?;
        chunk.validate_for_grant(grant, now_unix_ms)?;
        let staged = execution
            .files
            .get_mut(&chunk.path)
            .ok_or_else(|| anyhow::anyhow!("private-file path is not authorized"))?;
        anyhow::ensure!(
            staged.grant_id == chunk.grant_id
                && !staged.complete
                && chunk.offset == staged.next_offset,
            "private-file chunk is replayed or out of order"
        );
        let root = self.execution_root(execution_id)?;
        let path = self.file_path(execution_id, &chunk.path)?;
        create_parent_directories(&root, &path)?;
        let create = chunk.offset == 0;
        let mut file = open_stage_file(&path, create)?;
        anyhow::ensure!(file.metadata()?.len() == chunk.offset);
        file.seek(SeekFrom::Start(chunk.offset))?;
        file.write_all(&chunk.bytes)?;
        file.flush()?;
        file.sync_all()?;
        staged.next_offset = staged
            .next_offset
            .checked_add(chunk.bytes.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("private-file offset overflow"))?;
        if staged.next_offset == staged.spec.size_bytes {
            let mut verified = Zeroizing::new(Vec::with_capacity(staged.spec.size_bytes as usize));
            let mut reader = open_stage_file(&path, false)?;
            reader.seek(SeekFrom::Start(0))?;
            reader.read_to_end(&mut verified)?;
            grant.validate_complete_file(&staged.spec.path, &verified)?;
            drop(reader);
            drop(file);
            set_readonly_private_file(&path)?;
            staged.complete = true;
        }
        Ok(())
    }

    pub(super) fn mount_root(
        &self,
        execution_id: &str,
        now_unix_ms: u64,
    ) -> anyhow::Result<Option<PathBuf>> {
        let executions = self
            .executions
            .lock()
            .map_err(|_| anyhow::anyhow!("private-file grant state is unavailable"))?;
        let Some(execution) = executions.get(execution_id) else {
            return Ok(None);
        };
        anyhow::ensure!(!execution.grants.is_empty());
        for grant in execution.grants.values() {
            grant.validate_at(now_unix_ms)?;
        }
        anyhow::ensure!(
            execution.files.values().all(|file| file.complete),
            "private-file grant is incomplete"
        );
        let directory = self.execution_root(execution_id)?;
        seal_mount_tree(&directory)?;
        let metadata = fs::symlink_metadata(&directory)?;
        let canonical = directory.canonicalize()?;
        anyhow::ensure!(
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && canonical == directory
                && canonical.starts_with(&self.root),
            "private-file staging root is unsafe"
        );
        Ok(Some(canonical))
    }

    pub(super) fn expiry(&self, execution_id: &str) -> anyhow::Result<Option<u64>> {
        let executions = self
            .executions
            .lock()
            .map_err(|_| anyhow::anyhow!("private-file grant state is unavailable"))?;
        Ok(executions.get(execution_id).and_then(|execution| {
            execution
                .grants
                .values()
                .map(|grant| grant.expires_at_unix_ms)
                .min()
        }))
    }

    pub(super) fn mark_started(&self, execution_id: &str) -> anyhow::Result<()> {
        let mut executions = self
            .executions
            .lock()
            .map_err(|_| anyhow::anyhow!("private-file grant state is unavailable"))?;
        if let Some(execution) = executions.get_mut(execution_id) {
            anyhow::ensure!(
                execution.files.values().all(|file| file.complete),
                "private-file grant is incomplete"
            );
            write_started_marker(&self.root, execution_id)?;
            execution.mounted = true;
        }
        Ok(())
    }

    pub(super) fn validate_workspace(
        &self,
        execution_id: &str,
        workspace: &crate::execution::contract::WorkspaceRef,
    ) -> anyhow::Result<()> {
        let executions = self
            .executions
            .lock()
            .map_err(|_| anyhow::anyhow!("private-file grant state is unavailable"))?;
        if let Some(execution) = executions.get(execution_id) {
            anyhow::ensure!(
                execution
                    .grants
                    .values()
                    .all(|grant| &grant.workspace == workspace),
                "private-file grant belongs to another workspace"
            );
        }
        Ok(())
    }

    fn execution_root(&self, execution_id: &str) -> anyhow::Result<PathBuf> {
        validate_uuid(execution_id)?;
        Ok(self.root.join(execution_id))
    }

    pub(super) fn try_owner_lock(&self, execution_id: &str) -> anyhow::Result<File> {
        validate_uuid(execution_id)?;
        let path = self.root.join(format!("{execution_id}.owner"));
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(path)?;
        anyhow::ensure!(
            file.metadata()?.is_file(),
            "private-file owner lock is unsafe"
        );
        fs2::FileExt::try_lock_exclusive(&file)?;
        Ok(file)
    }

    fn file_path(&self, execution_id: &str, relative_path: &str) -> anyhow::Result<PathBuf> {
        crate::execution::remote::private_files::validate_private_path(relative_path)?;
        Ok(self.execution_root(execution_id)?.join(relative_path))
    }
}
