//! Personal launch history. Never written to a shared workspace manifest.
use std::{collections::BTreeMap, fs, io::ErrorKind};

use serde::{Deserialize, Serialize};

use super::{LocalWorkspaceSummary, WorkspaceManager};

#[derive(Serialize, Deserialize)]
struct History {
    version: u32,
    #[serde(default)]
    heads: BTreeMap<uuid::Uuid, String>,
    #[serde(default)]
    tokens: BTreeMap<uuid::Uuid, String>,
    #[serde(default)]
    titles: BTreeMap<uuid::Uuid, String>,
    #[serde(default)]
    identities: BTreeMap<uuid::Uuid, super::manager::RelocationIdentity>,
    opened: BTreeMap<uuid::Uuid, chrono::DateTime<chrono::Utc>>,
}

impl WorkspaceManager {
    fn read_history(&self) -> anyhow::Result<History> {
        let path = self.home.join("workspace-recency.json");
        match fs::read(path) {
            Ok(bytes) => {
                let history: History = serde_json::from_slice(&bytes)?;
                anyhow::ensure!(
                    history.version == 1,
                    "unsupported workspace history version"
                );
                Ok(history)
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(History {
                version: 1,
                heads: BTreeMap::new(),
                tokens: BTreeMap::new(),
                titles: BTreeMap::new(),
                identities: BTreeMap::new(),
                opened: BTreeMap::new(),
            }),
            Err(error) => Err(error.into()),
        }
    }

    /// Call only after the app has successfully connected. Listing and probing
    /// workspaces must not change launch order.
    pub fn record_successful_open(&self, id: uuid::Uuid) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.association_path(id).is_file(),
            "workspace is not associated"
        );
        // Unlike repository operations, this tiny index transaction waits for
        // concurrent app windows rather than dropping a successful-open event.
        fs::create_dir_all(&self.home)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.home.join(".workspace-recency.lock"))?;
        lock.lock()?;
        let mut history = match self.read_history() {
            Ok(history) => history,
            Err(error) if error.downcast_ref::<serde_json::Error>().is_some() => {
                // Preserve damaged bytes for recovery; never overwrite an
                // unknown future schema or treat a permission failure as empty.
                fs::rename(
                    self.home.join("workspace-recency.json"),
                    self.home.join(format!(
                        "workspace-recency-damaged-{}.json",
                        uuid::Uuid::new_v4()
                    )),
                )?;
                History {
                    version: 1,
                    heads: BTreeMap::new(),
                    tokens: BTreeMap::new(),
                    titles: BTreeMap::new(),
                    identities: BTreeMap::new(),
                    opened: BTreeMap::new(),
                }
            }
            Err(error) => return Err(error),
        };
        history.opened.insert(id, chrono::Utc::now());
        if let Ok(association) = super::WorkspaceAssociation::read(&self.association_path(id), id) {
            let _repository_lock =
                crate::core::gitops::lock_repository(&association.planning_repo_path)?;
            let head = super::git::run(
                Some(&association.planning_repo_path),
                &["rev-parse", "HEAD"],
            )?;
            history.identities.remove(&id);
            if let Ok(identity) =
                super::manager::RelocationIdentity::capture(&association.planning_repo_path)
            {
                history.identities.insert(id, identity);
            }
            if let Ok(manifest) = super::WorkspaceManifest::read(&association.planning_repo_path)
                && let Some(title) = manifest.title
            {
                history.titles.insert(id, title);
            }
            let git_dir = super::git::run(
                Some(&association.planning_repo_path),
                &["rev-parse", "--absolute-git-dir"],
            )?;
            let marker = std::path::PathBuf::from(String::from_utf8(git_dir.stdout)?.trim())
                .join("koolade-relocation");
            let marker_lock = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(marker.with_extension("lock"))?;
            marker_lock.lock()?;
            let token = match fs::read_to_string(&marker) {
                Ok(value) => {
                    anyhow::ensure!(
                        uuid::Uuid::parse_str(value.trim()).is_ok(),
                        "invalid workspace relocation marker"
                    );
                    value.trim().to_owned()
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    let token = uuid::Uuid::new_v4().to_string();
                    crate::artifacts::atomic_write(&marker, &token)?;
                    token
                }
                Err(error) => return Err(error.into()),
            };
            history.tokens.insert(id, token);
            history
                .heads
                .insert(id, String::from_utf8(head.stdout)?.trim().into());
        }
        crate::artifacts::atomic_write(
            &self.home.join("workspace-recency.json"),
            &serde_json::to_string_pretty(&history)?,
        )?;
        Ok(())
    }

    pub fn unavailable_workspace_label(&self, id: uuid::Uuid) -> (String, bool) {
        let title = self
            .read_history()
            .ok()
            .and_then(|mut history| history.titles.remove(&id))
            .unwrap_or_else(|| format!("Workspace {}", &id.to_string()[..8]));
        let missing = super::WorkspaceAssociation::read(&self.association_path(id), id).is_ok_and(
            |association| {
                fs::symlink_metadata(association.planning_repo_path)
                    .is_err_and(|error| error.kind() == ErrorKind::NotFound)
            },
        );
        (title, missing)
    }

    pub(super) fn relocation_identity(
        &self,
        id: uuid::Uuid,
    ) -> anyhow::Result<Option<super::manager::RelocationIdentity>> {
        Ok(self.read_history()?.identities.remove(&id))
    }

    pub(super) fn reconnect_token(&self, id: uuid::Uuid) -> anyhow::Result<Option<String>> {
        Ok(self.read_history()?.tokens.remove(&id))
    }

    pub(super) fn last_opened_head(&self, id: uuid::Uuid) -> anyhow::Result<Option<String>> {
        Ok(self.read_history()?.heads.remove(&id))
    }

    pub fn last_opened_dates(
        &self,
    ) -> anyhow::Result<BTreeMap<uuid::Uuid, chrono::DateTime<chrono::Utc>>> {
        Ok(self.read_history()?.opened)
    }

    pub fn sort_by_recent_open(
        &self,
        workspaces: &mut [LocalWorkspaceSummary],
    ) -> anyhow::Result<()> {
        let history = self.read_history()?;
        workspaces.sort_by(|left, right| {
            history
                .opened
                .get(&right.id)
                .cmp(&history.opened.get(&left.id))
                .then(left.title.cmp(&right.title))
                .then(left.id.cmp(&right.id))
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
