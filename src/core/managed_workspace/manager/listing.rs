use std::fs;

use super::{LocalWorkspaceSummary, WorkspaceManager};

#[derive(Default)]
pub struct WorkspaceLauncher {
    pub available: Vec<LocalWorkspaceSummary>,
    pub unavailable: Vec<uuid::Uuid>,
}

impl WorkspaceManager {
    /// Lightweight launcher routing only. No repository or Git inspection.
    pub fn has_local_associations(&self) -> bool {
        fs::read_dir(self.home.join("projects")).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && entry
                        .file_name()
                        .to_str()
                        .and_then(|name| uuid::Uuid::parse_str(name).ok())
                        .is_some_and(|id| self.association_path(id).is_file())
            })
        })
    }

    /// Return locally associated workspaces without creating or migrating data.
    pub fn list_local(&self) -> anyhow::Result<Vec<LocalWorkspaceSummary>> {
        let launcher = self.list_launcher()?;
        anyhow::ensure!(
            launcher.unavailable.is_empty(),
            "one or more workspaces need reconnecting"
        );
        Ok(launcher.available)
    }

    pub fn list_launcher(&self) -> anyhow::Result<WorkspaceLauncher> {
        let projects = self.home.join("projects");
        if !projects.is_dir() {
            return Ok(WorkspaceLauncher::default());
        }
        let mut workspaces = Vec::new();
        let mut unavailable = Vec::new();
        for entry in fs::read_dir(projects)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() || entry.file_name() == ".join.lock" {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(id) = uuid::Uuid::parse_str(&name) else {
                continue;
            };
            if id.hyphenated().to_string() != name || !self.association_path(id).is_file() {
                continue;
            }
            let workspace = match Self::open_unlocked(self, id) {
                Ok(workspace) => workspace,
                Err(_) => {
                    unavailable.push(id);
                    continue;
                }
            };
            let title = crate::core::state::PlannerState::load_with_store(
                &workspace.planning_repo,
                &workspace.store,
            )
            .map(|state| state.title)
            .unwrap_or_else(|_| format!("Workspace {id}"));
            workspaces.push(LocalWorkspaceSummary {
                id,
                title,
                planning_repo: workspace.planning_repo,
            });
        }
        workspaces.sort_by(|left, right| left.title.cmp(&right.title).then(left.id.cmp(&right.id)));
        unavailable.sort();
        Ok(WorkspaceLauncher {
            available: workspaces,
            unavailable,
        })
    }
}
