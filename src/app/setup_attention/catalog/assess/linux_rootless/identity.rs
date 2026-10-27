use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, HostCapabilityMatrixV1, HostKind,
    ImageArchitectureV1 as Architecture, LinuxRootlessRecipeV1 as Recipe,
    PrerequisiteCapability as Capability, ProbeEvidenceV1 as Evidence,
    ProbeObservationV1 as Observation, SupportClaim,
};

use super::facts::host_fact;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RecipeIdentity {
    Candidate,
    Unsupported,
    Unknown,
}

pub(super) fn recipe_identity(
    host: HostKind,
    generation: u64,
    matrix: &HostCapabilityMatrixV1,
    evidence: &[Evidence],
) -> RecipeIdentity {
    let Some(distribution_fact) =
        host_fact(evidence, host, generation, Capability::LinuxDistribution)
    else {
        return RecipeIdentity::Unknown;
    };
    let Some(recipe_fact) = host_fact(evidence, host, generation, Capability::LinuxRootlessRecipe)
    else {
        return RecipeIdentity::Unknown;
    };
    let distribution_matches = distribution_fact.state == State::Ready
        && distribution_fact.code == Code::LinuxDistributionDetected
        && matches!(
            &distribution_fact.observation,
            Some(Observation::LinuxDistribution {
                distribution,
                version,
            }) if *distribution == matrix.linux_distribution_id
                && *version == matrix.linux_distribution_version
        )
        && matrix.linux_distribution == State::Ready;
    if !distribution_matches {
        return RecipeIdentity::Unknown;
    }

    let expected_recipe = Recipe::for_host(
        matrix.linux_distribution_id,
        matrix.linux_distribution_version,
        matrix.observed_host_architecture,
    );
    let exact_matrix = expected_recipe.is_some()
        && matrix.native_controller == SupportClaim::Experimental
        && matrix.linux_distribution_support == Some(SupportClaim::Experimental);
    if exact_matrix
        && recipe_fact.state == State::Ready
        && recipe_fact.code == Code::LinuxRootlessRecipeCandidate
        && matches!(
            &recipe_fact.observation,
            Some(Observation::LinuxRootlessRecipe {
                candidate,
                distribution_state: State::Ready,
            }) if *candidate == expected_recipe
        )
    {
        return RecipeIdentity::Candidate;
    }

    let known_identity =
        matrix.linux_distribution_id.is_some() && matrix.linux_distribution_version.is_some();
    let non_candidate_matrix = expected_recipe.is_none();
    let expected_distribution_support = if Recipe::for_host(
        matrix.linux_distribution_id,
        matrix.linux_distribution_version,
        Architecture::Amd64,
    )
    .is_some()
    {
        SupportClaim::Experimental
    } else {
        SupportClaim::Unverified
    };
    if known_identity
        && non_candidate_matrix
        && matrix.native_controller == SupportClaim::Unverified
        && matrix.linux_distribution_support == Some(expected_distribution_support)
        && recipe_fact.state == State::Unknown
        && recipe_fact.code == Code::LinuxRootlessRecipeUnverified
        && matches!(
            &recipe_fact.observation,
            Some(Observation::LinuxRootlessRecipe {
                candidate: None,
                distribution_state: State::Ready,
            })
        )
    {
        RecipeIdentity::Unsupported
    } else {
        RecipeIdentity::Unknown
    }
}
