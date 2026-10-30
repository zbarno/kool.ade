//! Creation bound to an onboarding transaction's preallocated workspace ID.
use super::*;

impl WorkspaceManager {
    /// The caller durably records the ID before starting. Intent is persisted
    /// outside the destination before publishing that directory, so an
    /// interruption before its internal journal exists remains recoverable.
    pub fn create_identified(
        &self,
        id: uuid::Uuid,
        title: &str,
    ) -> anyhow::Result<ManagedWorkspace> {
        validate_title(title)?;
        let projects = self.home.join("projects");
        fs::create_dir_all(&projects)?;
        let _global_lock = locking::lock(&projects.join(".workspace-create.lock"))?;
        let project_dir = self.project_dir(id);
        let intent = projects.join(format!(".creation-{id}.json"));
        let journal = journal_path(self, id);
        let exists = match fs::symlink_metadata(&project_dir) {
            Ok(metadata) => {
                anyhow::ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "workspace location is not a regular directory"
                );
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if intent.exists() {
            let pending = read_journal(&intent, id)?;
            anyhow::ensure!(
                pending.title == title,
                "creation request does not match saved intent"
            );
        } else if exists {
            // Do not put even a lock file into an unrelated directory.
            anyhow::ensure!(
                journal.is_file() || self.association_path(id).is_file(),
                "workspace location is occupied without a recoverable creation record"
            );
        } else {
            crate::artifacts::atomic_create_bytes(
                &intent,
                &serde_json::to_vec(&CreationJournal {
                    schema_version: 1,
                    workspace_id: id,
                    title: title.to_owned(),
                })?,
            )?;
        }
        if !exists {
            fs::create_dir(&project_dir)?;
        }
        if !journal.is_file() && !self.association_path(id).is_file() {
            anyhow::ensure!(
                intent.is_file() && fs::read_dir(&project_dir)?.next().is_none(),
                "workspace location contains unexpected files; nothing was replaced"
            );
            write_journal(self, id, title)?;
        }
        if journal.is_file() {
            let pending = read_journal(&journal, id)?;
            anyhow::ensure!(
                pending.title == title,
                "creation request does not match pending workspace"
            );
        }
        let _workspace_lock = locking::lock(&project_dir.join(".workspace.lock"))?;
        let workspace = if journal.is_file() {
            finish_creation(
                self,
                id,
                title,
                #[cfg(test)]
                None,
            )?
        } else {
            Self::open_unlocked(self, id)?
        };
        if intent.is_file() {
            fs::remove_file(intent)?;
        }
        Ok(workspace)
    }
}
