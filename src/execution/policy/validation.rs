use super::ExecutionPolicyV1;
use crate::execution::{contract::ExecutionModeV1, contract::ExecutionRequestV1};

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
