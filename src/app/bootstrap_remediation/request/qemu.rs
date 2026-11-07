use crate::app::{bootstrap_repair_policy::Host, setup_attention::catalog::LinuxRootlessRecipeV1};

use super::OperationParameters;

pub(super) fn validate_request(
    host: Host,
    prerequisite_id: &str,
    os_version: &str,
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        host == Host::Linux
            && prerequisite_id == "setup:computer:qemu"
            && LinuxRootlessRecipeV1::from_os_version(os_version) == Some(recipe)
            && is_supported_recipe(recipe),
        "QEMU operation requires one supported exact APT distro recipe"
    );
    Ok(())
}

pub(super) fn parameters(os_version: &str) -> anyhow::Result<OperationParameters> {
    let recipe = LinuxRootlessRecipeV1::from_os_version(os_version)
        .ok_or_else(|| anyhow::anyhow!("Linux host identity has no supported QEMU APT recipe"))?;
    anyhow::ensure!(
        is_supported_recipe(recipe),
        "QEMU installation has no trusted package recipe for this distro"
    );
    Ok(OperationParameters::InstallLinuxQemu { recipe })
}

fn is_supported_recipe(recipe: LinuxRootlessRecipeV1) -> bool {
    matches!(
        recipe,
        LinuxRootlessRecipeV1::Debian13Amd64
            | LinuxRootlessRecipeV1::Ubuntu2404Amd64
            | LinuxRootlessRecipeV1::PopOs2404Amd64
    )
}
