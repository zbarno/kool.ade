use super::PlannerState;

impl PlannerState {
    /// Operator-local configuration directory inside this installation's
    /// private project store. It is never written to the code repository.
    pub fn mcp_config_dir(&self) -> std::path::PathBuf {
        self.planning_store.root.clone()
    }
}
