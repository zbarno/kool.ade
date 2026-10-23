use super::super::ExecutionPolicyV1;
use crate::execution::{contract::ExecutionModeV1, contract::ExecutionRequestV1};
use sha2::Digest;

impl ExecutionPolicyV1 {
    pub fn validate_request(&self, request: &ExecutionRequestV1) -> anyhow::Result<()> {
        self.validate()?;
        anyhow::ensure!(
            self.mode == request.mode,
            "policy mode does not match request mode"
        );
        anyhow::ensure!(
            request
                .environment_allowlist
                .is_subset(&self.environment_names),
            "request environment exceeds policy"
        );
        anyhow::ensure!(
            request
                .allowed_service_ports
                .is_subset(&self.network_service_ports),
            "request service ports exceed policy"
        );
        if let Some(approved) = self.approved_recipe_command() {
            let mut exact_argv = Vec::with_capacity(approved.argv.len() + 1);
            exact_argv.push(approved.executable_id.clone());
            exact_argv.extend(approved.argv.iter().cloned());
            anyhow::ensure!(
                request.argv == exact_argv
                    && request.working_directory_relative == approved.working_directory_relative
                    && request.environment_allowlist == approved.allowed_environment_names
                    && match (&approved.stdin_script_sha256, &request.stdin) {
                        (Some(expected), Some(script)) => {
                            format!("{:x}", sha2::Sha256::digest(script.as_bytes())) == *expected
                        }
                        (None, None) => true,
                        _ => false,
                    },
                "execution request differs from the explicitly approved project recipe"
            );
        } else {
            anyhow::ensure!(
                !self
                    .filesystem_grants()
                    .contains(&crate::execution::policy::FilesystemGrantV1::WorkspaceScopedWrite),
                "scoped workspace writes require an approved exact recipe command"
            );
        }
        let requested = &request.resource_budget;
        let limit = &self.resource_limits;
        anyhow::ensure!(
            requested.cpu_millis <= limit.cpu_millis
                && requested.memory_bytes <= limit.memory_bytes
                && requested.process_count <= limit.process_count
                && requested.temporary_disk_bytes <= limit.temporary_disk_bytes
                && requested.output_bytes <= limit.output_bytes,
            "request resources exceed policy"
        );
        if request.mode != ExecutionModeV1::RuntimeQa {
            anyhow::ensure!(
                request.allowed_service_ports.is_empty(),
                "service ports are QA-only"
            );
        }
        Ok(())
    }
}
