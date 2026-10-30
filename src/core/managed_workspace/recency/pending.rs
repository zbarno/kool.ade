//! Durable successful-open events survive shutdown before background Git work.
use super::*;

#[derive(Serialize, Deserialize)]
struct PendingOpen {
    version: u32,
    workspace_id: uuid::Uuid,
    opened_at: chrono::DateTime<chrono::Utc>,
}

impl WorkspaceManager {
    /// Save a small private event before scheduling repository-dependent work.
    /// This does not acquire a repository or history lock.
    pub fn queue_successful_open(&self, id: uuid::Uuid) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.association_path(id).is_file(),
            "workspace is not associated"
        );
        let event = PendingOpen {
            version: 1,
            workspace_id: id,
            opened_at: chrono::Utc::now(),
        };
        let path = self
            .home
            .join("pending-workspace-opens")
            .join(format!("{}.json", uuid::Uuid::new_v4()));
        crate::artifacts::atomic_write(&path, &serde_json::to_string(&event)?)
    }

    /// Retry only recorded successful connections. Failed events remain intact.
    pub fn flush_pending_opens(&self) -> anyhow::Result<()> {
        let entries = match fs::read_dir(self.home.join("pending-workspace-opens")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let mut failure = None;
        for entry in entries {
            let result = (|| -> anyhow::Result<()> {
                let path = entry?.path();
                if path.extension().is_none_or(|ext| ext != "json") {
                    return Ok(());
                }
                let bytes = match fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
                    Err(error) => return Err(error.into()),
                };
                let event: PendingOpen = serde_json::from_slice(&bytes)?;
                anyhow::ensure!(event.version == 1, "unsupported pending-open version");
                self.record_successful_open_at(event.workspace_id, event.opened_at)?;
                match fs::remove_file(path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error.into()),
                }
            })();
            if let Err(error) = result {
                failure = Some(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
}
