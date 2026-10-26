mod candidate;
mod source;

#[cfg(test)]
#[path = "apt/tests.rs"]
mod tests;

use crate::app::bootstrap_remediation::{TrustedArgument, VerifiedArtifact};
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

pub(super) fn package_arguments(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<Vec<TrustedArgument>> {
    ensure_apt_recipe(recipe)?;
    let mut arguments = TrustedArgument::apt_source_options();
    arguments.extend([
        TrustedArgument::AptInstall,
        TrustedArgument::AptAssumeYes,
        TrustedArgument::AptNoUpgrade,
        TrustedArgument::AptNoRemove,
        TrustedArgument::AptNoInstallRecommends,
        TrustedArgument::AptDockerCe,
        TrustedArgument::AptDockerCeCli,
        TrustedArgument::AptContainerdIo,
        TrustedArgument::AptDockerRootlessExtras,
        TrustedArgument::AptUidmap,
        TrustedArgument::AptDbusUserSession,
    ]);
    Ok(arguments)
}

pub(super) fn verify_package_source(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<()> {
    ensure_apt_recipe(recipe)?;
    #[cfg(target_os = "linux")]
    {
        source::check_docker_source_configuration(recipe)?;
        source::check_docker_keyring()?;
        candidate::check_docker_candidates(recipe)?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        anyhow::bail!("Linux rootless Docker packages are Linux-only")
    }
}

pub(super) fn validate_package_source_bootstrap(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<()> {
    ensure_apt_recipe(recipe)?;
    source::docker_source_bootstrap(recipe).map(|_| ())
}

pub(super) fn package_source_artifacts(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<(bool, Option<VerifiedArtifact>, Option<VerifiedArtifact>)> {
    ensure_apt_recipe(recipe)?;
    let setup = source::docker_source_bootstrap(recipe)?;
    let (key, sources) = source::docker_source_file_artifacts(recipe, setup)?;
    Ok((setup.create_keyrings_directory, key, sources))
}

fn ensure_apt_recipe(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(
            recipe,
            LinuxRootlessRecipeV1::Debian13Amd64
                | LinuxRootlessRecipeV1::Ubuntu2404Amd64
                | LinuxRootlessRecipeV1::PopOs2404Amd64
        ),
        "APT package source requires an exact Debian, Ubuntu, or Pop!_OS recipe"
    );
    Ok(())
}
