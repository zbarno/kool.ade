use super::*;

impl ExecutionPolicyV1 {
    pub fn restrict_to(&self, ceiling: &Self) -> anyhow::Result<Self> {
        self.validate()?;
        ceiling.validate()?;
        anyhow::ensure!(
            self.mode == ceiling.mode,
            "cannot combine policies for different modes"
        );
        let approved_recipe_command = self.approved_recipe_command.clone().filter(|_| {
            ceiling
                .filesystem
                .contains(&FilesystemGrantV1::WorkspaceScopedWrite)
        });
        let mut command_capabilities = self
            .command_capabilities
            .intersection(&ceiling.command_capabilities)
            .cloned()
            .collect::<BTreeSet<_>>();
        if let Some(approved) = &approved_recipe_command {
            command_capabilities.insert(approved.executable_id.clone());
        }
        Ok(Self {
            schema_version: 1,
            authority: PolicyAuthorityV1::Application,
            mode: self.mode,
            filesystem: self
                .filesystem
                .intersection(&ceiling.filesystem)
                .copied()
                .collect(),
            workspace_write_scope: if ceiling
                .filesystem
                .contains(&FilesystemGrantV1::WorkspaceScopedWrite)
            {
                self.workspace_write_scope.clone()
            } else {
                None
            },
            approved_recipe_command,
            network_access: if self
                .network_service_ports
                .intersection(&ceiling.network_service_ports)
                .next()
                .is_some()
            {
                NetworkAccessV1::RuntimeQaServicePorts
            } else {
                NetworkAccessV1::Disabled
            },
            network_service_ports: self
                .network_service_ports
                .intersection(&ceiling.network_service_ports)
                .copied()
                .collect(),
            environment_names: self
                .environment_names
                .intersection(&ceiling.environment_names)
                .cloned()
                .collect(),
            command_capabilities,
            tool_capabilities: self
                .tool_capabilities
                .intersection(&ceiling.tool_capabilities)
                .cloned()
                .collect(),
            // Artifact paths are a bounded request selection inside the
            // verification capability; they are not a grant ceiling.
            artifact_paths: self.artifact_paths.clone(),
            resource_limits: self.resource_limits.restricted_by(&ceiling.resource_limits),
            run_as_non_root: true,
            host_secrets: false,
            privileged: false,
        })
    }
}
