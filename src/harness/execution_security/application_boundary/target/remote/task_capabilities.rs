use crate::persistence::execution_environment_settings::TaskCapabilityRequirements;

pub(super) fn verify_task_capabilities(
    _reported: &crate::execution::backend::BackendCapabilitiesV1,
    requirements: TaskCapabilityRequirements,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !requirements.requires_gpu,
        "GPU tasks are unavailable until supported runtime and device-grant policies are defined"
    );
    anyhow::ensure!(
        !requirements.requires_display,
        "Display tasks are unavailable until supported runtime, display-mode, and device-grant policies are defined"
    );
    Ok(())
}
