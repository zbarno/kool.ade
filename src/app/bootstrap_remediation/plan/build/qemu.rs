use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

use super::super::{
    ManagedPackageChange, PlanConsent, PlanEffect, PrivilegeLevel, RemediationPlan,
    RollbackGuidance,
};

pub(super) fn install_packages(
    plan: &mut RemediationPlan,
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(
            recipe,
            LinuxRootlessRecipeV1::Debian13Amd64
                | LinuxRootlessRecipeV1::Ubuntu2404Amd64
                | LinuxRootlessRecipeV1::PopOs2404Amd64
        ),
        "QEMU installation requires an exact supported APT distro recipe"
    );
    plan.effects.push(PlanEffect::InstallLinuxQemu);
    plan.package_changes.push(ManagedPackageChange::Qemu);
    plan.precondition_probe_ids = vec![
        "host:linux:package_manager".into(),
        "host:linux:rootless_recipe".into(),
    ];
    plan.verification_probe_ids = vec!["host:linux:qemu".into()];
    plan.required_consents
        .push(PlanConsent::QemuPackageInstallation);
    plan.privilege = PrivilegeLevel::Administrator;
    plan.rollback = RollbackGuidance::KeepInstalledPackagesForManualRecovery;
    plan.requires_consent = true;
    Ok(())
}
