use crate::execution::{backend::BackendCapabilitiesV1, contract::ResourceBudgetV1};

pub(super) fn verify_resource_availability(
    capabilities: &BackendCapabilitiesV1,
    resources: &ResourceBudgetV1,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        capabilities
            .available_cpu_millis
            .is_some_and(|available| available >= resources.cpu_millis)
            && capabilities
                .available_memory_bytes
                .is_some_and(|available| available >= resources.memory_bytes),
        "Remote worker does not have enough available CPU or memory"
    );
    Ok(())
}
