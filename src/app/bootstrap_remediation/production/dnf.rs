mod candidate;
mod key;
mod source;

#[cfg(test)]
#[path = "dnf/tests.rs"]
mod tests;

use crate::app::bootstrap_remediation::{TrustedArgument, VerifiedArtifact};
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

pub(crate) fn package_arguments(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<Vec<TrustedArgument>> {
    ensure_fedora_recipe(recipe)?;
    Ok(candidate::common_arguments()
        .into_iter()
        .chain([
            TrustedArgument::DnfInstall,
            TrustedArgument::DnfAssumeYes,
            TrustedArgument::DnfBest,
            TrustedArgument::DnfNoWeakDependencies,
            TrustedArgument::DnfDockerCe,
            TrustedArgument::DnfDockerCeCli,
            TrustedArgument::DnfContainerdIo,
            TrustedArgument::DnfDockerRootlessExtras,
            TrustedArgument::DnfShadowUtils,
            TrustedArgument::DnfSlirp4netns,
        ])
        .collect())
}

pub(crate) fn verify_package_source(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<()> {
    ensure_fedora_recipe(recipe)?;
    source::check_docker_repository_configuration()?;
    candidate::check_docker_candidates()
}

pub(crate) fn validate_package_source_bootstrap(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<()> {
    ensure_fedora_recipe(recipe)?;
    source::docker_repository_bootstrap().map(|_| ())
}

pub(crate) fn package_source_artifacts(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<(bool, Option<VerifiedArtifact>, Option<VerifiedArtifact>)> {
    ensure_fedora_recipe(recipe)?;
    let setup = source::docker_repository_bootstrap()?;
    let (key, repo) = source::docker_repository_file_artifacts(setup)?;
    Ok((false, key, repo))
}

pub(crate) fn make_cache_arguments(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<Vec<TrustedArgument>> {
    ensure_fedora_recipe(recipe)?;
    Ok(candidate::common_arguments()
        .into_iter()
        .chain([TrustedArgument::DnfMakeCache, TrustedArgument::DnfRefresh])
        .collect())
}

pub(crate) fn candidate_arguments(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<Vec<TrustedArgument>> {
    ensure_fedora_recipe(recipe)?;
    Ok(candidate::trusted_arguments())
}

pub(crate) fn validate_candidate_output(
    recipe: LinuxRootlessRecipeV1,
    output: &str,
) -> anyhow::Result<()> {
    ensure_fedora_recipe(recipe)?;
    anyhow::ensure!(
        candidate::candidates_have_approved_origins(output),
        "DNF candidates are missing or come from an unapproved signed repository"
    );
    Ok(())
}

pub(crate) fn validate_repository_files(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<()> {
    ensure_fedora_recipe(recipe)?;
    source::check_docker_repository_configuration()
}

pub(crate) fn package_source_ready(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<()> {
    verify_package_source(recipe)
}

fn ensure_fedora_recipe(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<()> {
    anyhow::ensure!(
        recipe == LinuxRootlessRecipeV1::Fedora45Amd64,
        "DNF package source requires the exact Fedora 45 x86_64 recipe"
    );
    Ok(())
}
