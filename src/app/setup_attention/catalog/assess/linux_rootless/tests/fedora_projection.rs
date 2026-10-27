use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, FindingStatus, HostKind,
    LinuxDistributionV1 as Distribution, LinuxDistributionVersionV1 as Version,
    LinuxPackageManagerV1 as PackageManager, LinuxRootlessRecipeV1 as Recipe,
    ProbeObservationV1 as Observation,
};

use super::super::{PACKAGES_ID, ParentDisposition, project};
use super::fixture::{GENERATION, candidate_evidence, candidate_matrix, set_fact};

#[test]
fn fedora_45_dnf_projection_keeps_package_setup_actionable() {
    let mut matrix = candidate_matrix();
    let fedora_version = Version {
        major: 45,
        minor: 0,
    };
    matrix.linux_distribution_id = Some(Distribution::Fedora);
    matrix.linux_distribution_version = Some(fedora_version);
    let mut evidence = candidate_evidence();
    set_fact(
        &mut evidence,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxDistribution,
        State::Ready,
        Code::LinuxDistributionDetected,
        Some(Observation::LinuxDistribution {
            distribution: Some(Distribution::Fedora),
            version: Some(fedora_version),
        }),
    );
    set_fact(
        &mut evidence,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxRootlessRecipe,
        State::Ready,
        Code::LinuxRootlessRecipeCandidate,
        Some(Observation::LinuxRootlessRecipe {
            candidate: Some(Recipe::Fedora45Amd64),
            distribution_state: State::Ready,
        }),
    );
    set_fact(
        &mut evidence,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxRootlessHelpers,
        State::Missing,
        Code::LinuxHostCheckMissing,
        Some(Observation::LinuxRootlessHelpers {
            newuidmap: false,
            newgidmap: true,
            rootlesskit: true,
            dockerd_rootless: true,
            setuptool: true,
            slirp4netns: Some(true),
        }),
    );
    set_fact(
        &mut evidence,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxPackageManager,
        State::Ready,
        Code::LinuxPackageManagerDetected,
        Some(Observation::LinuxPackageManager {
            expected: Some(PackageManager::Dnf),
            found: Some(PackageManager::Dnf),
        }),
    );

    let projection = project(HostKind::Linux, GENERATION, &matrix, &evidence, true);

    assert_eq!(projection.disposition, ParentDisposition::Candidate);
    assert_eq!(
        projection
            .findings
            .iter()
            .find(|finding| finding.id == PACKAGES_ID)
            .expect("Fedora package task")
            .status,
        FindingStatus::NeedsAttention
    );
}
