use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, Finding, FindingStatus, HostKind,
    LinuxDistributionV1 as Distribution, LinuxDistributionVersionV1 as Version,
    LinuxPackageManagerV1 as PackageManager, ProbeObservationV1 as Observation, SupportClaim,
};

use super::super::{
    PACKAGES_ID, ParentDisposition, ROOTLESS_PARENT_ID, SERVICE_ID, SUBIDS_ID, project,
    update_parent,
};
use super::fixture::{GENERATION, candidate_evidence, candidate_matrix, set_fact};

fn finding<'a>(findings: &'a [Finding], id: &str) -> &'a Finding {
    findings
        .iter()
        .find(|finding| finding.id == id)
        .expect("expected projected finding")
}

#[test]
fn exact_supported_identity_projects_missing_steps_in_dependency_order() {
    let ready = project(
        HostKind::Linux,
        GENERATION,
        &candidate_matrix(),
        &candidate_evidence(),
        true,
    );
    assert_eq!(ready.findings.len(), 1);
    assert_eq!(
        finding(&ready.findings, SERVICE_ID).status,
        FindingStatus::NeedsAttention
    );
    assert_eq!(
        finding(&ready.findings, SERVICE_ID).prerequisites,
        Vec::<String>::new()
    );

    let mut evidence = candidate_evidence();
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
    for capability in [
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxSubuid,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxSubgid,
    ] {
        set_fact(
            &mut evidence,
            capability,
            State::Missing,
            Code::LinuxHostCheckMissing,
            Some(Observation::LinuxSubordinateIds { configured: false }),
        );
    }
    let projection = project(
        HostKind::Linux,
        GENERATION,
        &candidate_matrix(),
        &evidence,
        true,
    );
    assert_eq!(projection.disposition, ParentDisposition::Candidate);
    assert_eq!(projection.findings.len(), 3);
    assert_eq!(
        finding(&projection.findings, PACKAGES_ID).status,
        FindingStatus::NeedsAttention
    );
    assert_eq!(
        finding(&projection.findings, PACKAGES_ID).prerequisites,
        Vec::<String>::new()
    );
    assert_eq!(
        finding(&projection.findings, SUBIDS_ID).status,
        FindingStatus::NeedsAttention
    );
    assert_eq!(
        finding(&projection.findings, SUBIDS_ID).prerequisites,
        vec![PACKAGES_ID.to_owned()]
    );
    assert_eq!(
        finding(&projection.findings, SERVICE_ID).status,
        FindingStatus::NeedsAttention
    );
    assert_eq!(
        finding(&projection.findings, SERVICE_ID).prerequisites,
        vec![PACKAGES_ID.to_owned(), SUBIDS_ID.to_owned()]
    );
}

#[test]
fn unknown_evidence_makes_repairs_manual_and_blocks_parent_operation() {
    let mut evidence = candidate_evidence();
    set_fact(
        &mut evidence,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxKernel,
        State::Unknown,
        Code::LinuxProbeInaccessible,
        None,
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
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxSubuid,
        State::Missing,
        Code::LinuxHostCheckMissing,
        Some(Observation::LinuxSubordinateIds { configured: false }),
    );
    let projection = project(
        HostKind::Linux,
        GENERATION,
        &candidate_matrix(),
        &evidence,
        true,
    );
    assert_eq!(projection.disposition, ParentDisposition::CandidateManual);
    assert!(
        projection
            .findings
            .iter()
            .all(|finding| finding.status == FindingStatus::Unverified)
    );
    let mut parent = Finding::action(
        ROOTLESS_PARENT_ID,
        "Configure secure Docker",
        "Rootful Docker is rejected.",
        "Configure rootless Docker.",
    );
    update_parent(&mut parent, projection.disposition);
    assert_eq!(parent.status, FindingStatus::Unverified);
}

#[test]
fn unknown_package_manager_and_service_manager_are_manual() {
    let mut evidence = candidate_evidence();
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
        State::Unknown,
        Code::LinuxProbeInaccessible,
        Some(Observation::LinuxPackageManager {
            expected: Some(PackageManager::AptGet),
            found: None,
        }),
    );
    set_fact(
        &mut evidence,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxServiceManager,
        State::Unknown,
        Code::LinuxProbeInaccessible,
        Some(Observation::LinuxServiceManager {
            runtime_directory_ready: false,
            private_socket_ready: false,
            bus_socket_ready: false,
        }),
    );
    let projection = project(
        HostKind::Linux,
        GENERATION,
        &candidate_matrix(),
        &evidence,
        true,
    );
    assert_eq!(
        finding(&projection.findings, PACKAGES_ID).status,
        FindingStatus::Unverified
    );
    assert_eq!(
        finding(&projection.findings, SERVICE_ID).status,
        FindingStatus::Unverified
    );
    assert!(
        !projection
            .findings
            .iter()
            .any(|finding| finding.status == FindingStatus::NeedsAttention)
    );
}

#[test]
fn other_known_distributions_are_unsupported_and_receive_no_child_operations() {
    let mut matrix = candidate_matrix();
    matrix.native_controller = SupportClaim::Unverified;
    matrix.linux_distribution_id = Some(Distribution::Arch);
    matrix.linux_distribution_version = Some(Version {
        major: 2025,
        minor: 10,
    });
    matrix.linux_distribution_support = Some(SupportClaim::Unverified);
    let mut evidence = candidate_evidence();
    set_fact(
        &mut evidence,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxDistribution,
        State::Ready,
        Code::LinuxDistributionDetected,
        Some(Observation::LinuxDistribution {
            distribution: Some(Distribution::Arch),
            version: Some(Version {
                major: 2025,
                minor: 10,
            }),
        }),
    );
    set_fact(
        &mut evidence,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxPackageManager,
        State::Ready,
        Code::LinuxPackageManagerDetected,
        Some(Observation::LinuxPackageManager {
            expected: Some(PackageManager::Pacman),
            found: Some(PackageManager::Pacman),
        }),
    );
    set_fact(
        &mut evidence,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxRootlessRecipe,
        State::Unknown,
        Code::LinuxRootlessRecipeUnverified,
        Some(Observation::LinuxRootlessRecipe {
            candidate: None,
            distribution_state: State::Ready,
        }),
    );
    let projection = project(HostKind::Linux, GENERATION, &matrix, &evidence, true);
    assert_eq!(projection.disposition, ParentDisposition::Unsupported);
    assert!(projection.findings.is_empty());
    let mut parent = Finding::action(
        ROOTLESS_PARENT_ID,
        "Configure secure Docker",
        "Rootful Docker is rejected.",
        "Configure rootless Docker.",
    );
    update_parent(&mut parent, projection.disposition);
    assert_eq!(parent.status, FindingStatus::Unsupported);
    assert!(parent.next_step.contains("manually"));
}

#[test]
fn stale_recipe_evidence_and_missing_parent_suppress_child_operations() {
    let evidence = candidate_evidence();
    let projection = project(
        HostKind::Linux,
        GENERATION + 1,
        &candidate_matrix(),
        &evidence,
        true,
    );
    assert_eq!(projection.disposition, ParentDisposition::Unknown);
    assert!(projection.findings.is_empty());
    let mut parent = Finding::action(
        ROOTLESS_PARENT_ID,
        "Configure secure Docker",
        "Rootful Docker is rejected.",
        "Configure rootless Docker.",
    );
    update_parent(&mut parent, projection.disposition);
    assert_eq!(parent.status, FindingStatus::Unverified);

    let projection = project(
        HostKind::Linux,
        GENERATION,
        &candidate_matrix(),
        &evidence,
        false,
    );
    assert_eq!(projection.disposition, ParentDisposition::NotApplicable);
    assert!(projection.findings.is_empty());
}
