use super::{
    ApprovedRecipeCommandV1, ExecutionModeV1, ExecutionPolicyV1, FilesystemGrantV1,
    ResourceBudgetV1, WorkspaceWriteScopeV1,
};
use std::collections::BTreeSet;

impl ExecutionPolicyV1 {
    pub fn workspace_write_scope(&self) -> Option<&WorkspaceWriteScopeV1> {
        self.workspace_write_scope.as_ref()
    }

    pub fn approved_recipe_command(&self) -> Option<&ApprovedRecipeCommandV1> {
        self.approved_recipe_command.as_ref()
    }

    pub fn mode(&self) -> ExecutionModeV1 {
        self.mode
    }

    pub fn command_capabilities(&self) -> &BTreeSet<String> {
        &self.command_capabilities
    }

    pub fn filesystem_grants(&self) -> &BTreeSet<FilesystemGrantV1> {
        &self.filesystem
    }

    pub fn network_service_ports(&self) -> &BTreeSet<u16> {
        &self.network_service_ports
    }

    pub fn tool_capabilities(&self) -> &BTreeSet<String> {
        &self.tool_capabilities
    }

    pub fn resource_limits(&self) -> &ResourceBudgetV1 {
        &self.resource_limits
    }

    pub fn artifact_paths(&self) -> &[String] {
        &self.artifact_paths
    }
}
