use super::PlannerState;

impl PlannerState {
    /// Operator-local configuration directory for this checkout's managed
    /// workspace. Files stored here (MCP credentials, operator prefs) live
    /// outside every Git repository and are never synchronized or shared.
    pub fn mcp_config_dir(&self) -> Option<std::path::PathBuf> {
        let manager = crate::core::managed_workspace::WorkspaceManager::from_state_root();
        let workspace = manager
            .workspace_for_checkout(&self.repo_root)
            .ok()
            .flatten()?;
        Some(manager.project_dir(workspace.id))
    }
}
