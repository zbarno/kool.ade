use std::time::Duration;

use crate::app::{
    bootstrap_remediation::{
        OperationRegistry, RemediationPlan, TrustedArgument, TrustedInvocation, TrustedProgram,
        VerifiedArtifact,
    },
    bootstrap_repair_policy::{Host, Operation},
    setup_attention::catalog::LinuxRootlessRecipeV1,
};

use super::{apt, dnf, subids};

const PACKAGE_OPERATION_ID: &str = "setup:linux:rootless:packages";
const SUBIDS_OPERATION_ID: &str = "setup:linux:rootless:subids";
const SERVICE_OPERATION_ID: &str = "setup:linux:rootless:service";
pub(crate) struct ProductionOperationRegistry;

pub(crate) struct PackageSourceArtifacts {
    pub(crate) create_keyrings_directory: bool,
    pub(crate) key: Option<VerifiedArtifact>,
    pub(crate) sources: Option<VerifiedArtifact>,
}

pub(crate) fn validate_before_elevated_launch(
    invocation: &TrustedInvocation,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        invocation.host() == Host::Linux,
        "rootless Docker elevation is Linux-only"
    );
    match invocation.program() {
        TrustedProgram::AptGet => {
            let recipe = current_linux_recipe()?;
            apt::verify_package_source(recipe)
        }
        TrustedProgram::Dnf => {
            let recipe = current_linux_recipe()?;
            dnf::verify_package_source(recipe)
        }
        TrustedProgram::LinuxRootlessPackageSetup => {
            let recipe = invocation.rootless_package_recipe()?;
            validate_package_source_bootstrap(recipe)
        }
        TrustedProgram::Usermod => {
            let (uid, name) = invocation.current_user_identity()?;
            subids::validate_current_mappings_for_uid(uid, name)
        }
        _ => anyhow::bail!("elevated launch is not approved for this trusted program"),
    }
}

pub(crate) fn package_source_artifacts(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<PackageSourceArtifacts> {
    let (create_keyrings_directory, key, sources) = match recipe {
        LinuxRootlessRecipeV1::Debian13Amd64
        | LinuxRootlessRecipeV1::Ubuntu2404Amd64
        | LinuxRootlessRecipeV1::PopOs2404Amd64 => apt::package_source_artifacts(recipe)?,
        LinuxRootlessRecipeV1::Fedora45Amd64 => dnf::package_source_artifacts(recipe)?,
    };
    Ok(PackageSourceArtifacts {
        create_keyrings_directory,
        key,
        sources,
    })
}

pub(crate) fn package_install_arguments(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<Vec<TrustedArgument>> {
    match recipe {
        LinuxRootlessRecipeV1::Debian13Amd64
        | LinuxRootlessRecipeV1::Ubuntu2404Amd64
        | LinuxRootlessRecipeV1::PopOs2404Amd64 => apt::package_arguments(recipe),
        LinuxRootlessRecipeV1::Fedora45Amd64 => dnf::package_arguments(recipe),
    }
}

pub(crate) fn production_package_source_ready(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<()> {
    verify_package_source(recipe)
}

pub(crate) fn validate_package_source_bootstrap(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<()> {
    match recipe {
        LinuxRootlessRecipeV1::Debian13Amd64
        | LinuxRootlessRecipeV1::Ubuntu2404Amd64
        | LinuxRootlessRecipeV1::PopOs2404Amd64 => apt::validate_package_source_bootstrap(recipe),
        LinuxRootlessRecipeV1::Fedora45Amd64 => dnf::validate_package_source_bootstrap(recipe),
    }
}

fn verify_package_source(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<()> {
    match recipe {
        LinuxRootlessRecipeV1::Debian13Amd64
        | LinuxRootlessRecipeV1::Ubuntu2404Amd64
        | LinuxRootlessRecipeV1::PopOs2404Amd64 => apt::verify_package_source(recipe),
        LinuxRootlessRecipeV1::Fedora45Amd64 => dnf::verify_package_source(recipe),
    }
}

impl OperationRegistry for ProductionOperationRegistry {
    fn compile(&self, plan: &RemediationPlan) -> anyhow::Result<TrustedInvocation> {
        validate_rootless_plan(plan)?;
        match plan.operation {
            Operation::InstallLinuxRootlessPackages => {
                let recipe = recipe_from_os_version(&plan.os_version)?;
                TrustedInvocation::new_for_current_user_systemd(
                    Host::Linux,
                    TrustedProgram::LinuxRootlessPackageSetup,
                    vec![TrustedArgument::LinuxRootlessRecipe(recipe)],
                    Duration::from_secs(300),
                )
            }
            Operation::ConfigureLinuxSubordinateIds => {
                let account = subids::verify_free_range_for_current_user()?;
                TrustedInvocation::new_for_current_user_systemd(
                    Host::Linux,
                    TrustedProgram::Usermod,
                    subids::arguments_for(&account),
                    Duration::from_secs(30),
                )
            }
            Operation::InstallLinuxRootlessService => {
                TrustedInvocation::new_for_current_user_systemd(
                    Host::Linux,
                    TrustedProgram::LinuxRootlessService,
                    Vec::<TrustedArgument>::new(),
                    Duration::from_secs(120),
                )
            }
            _ => anyhow::bail!("production rootless registry does not implement this operation"),
        }
    }
}

fn validate_rootless_plan(plan: &RemediationPlan) -> anyhow::Result<()> {
    plan.validate()?;
    anyhow::ensure!(
        plan.host == Host::Linux,
        "rootless Docker registry is Linux-only"
    );
    recipe_from_os_version(&plan.os_version)?;
    let expected_id = match plan.operation {
        Operation::InstallLinuxRootlessPackages => PACKAGE_OPERATION_ID,
        Operation::ConfigureLinuxSubordinateIds => SUBIDS_OPERATION_ID,
        Operation::InstallLinuxRootlessService => SERVICE_OPERATION_ID,
        _ => anyhow::bail!("production rootless registry does not implement this operation"),
    };
    anyhow::ensure!(
        plan.prerequisite_id == expected_id,
        "rootless Docker operation does not match its canonical task identity"
    );
    Ok(())
}

fn current_linux_recipe() -> anyhow::Result<LinuxRootlessRecipeV1> {
    let identity = crate::app::bootstrap_remediation::identity::HostIdentity::current()?;
    recipe_from_os_version(identity.os_version())
}

fn recipe_from_os_version(os_version: &str) -> anyhow::Result<LinuxRootlessRecipeV1> {
    LinuxRootlessRecipeV1::from_os_version(os_version)
        .ok_or_else(|| anyhow::anyhow!("Linux host identity has no exact rootless recipe"))
}

#[cfg(test)]
#[path = "registry/tests.rs"]
mod tests;
