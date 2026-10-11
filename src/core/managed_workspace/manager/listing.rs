use std::fs;

use super::{LocalWorkspaceSummary, WorkspaceManager};

impl WorkspaceManager {
    /// Return locally associated workspaces without creating or migrating data.
    pub fn list_local(&self) -> anyhow::Result<Vec<LocalWorkspaceSummary>> {
        let projects = self.home.join("projects");
        if !projects.is_dir() {
            return Ok(Vec::new());
        }
        let mut workspaces = Vec::new();
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
            let workspace = self.open(id).map_err(|error| {
                anyhow::anyhow!("local workspace {id} could not be opened: {error:#}")
            })?;
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
        Ok(workspaces)
    }
}
