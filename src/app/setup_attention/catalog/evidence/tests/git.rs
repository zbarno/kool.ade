use crate::{
    app::setup_attention::catalog::{
        self, EvidenceCode, EvidenceState, GitProbeV1, HostKind, PrerequisiteCapability,
        PrerequisiteDomain, PrerequisiteSubject,
    },
    execution::docker::environment::{DockerEnvironmentState, DockerEnvironmentStatus},
};

fn unavailable_docker() -> DockerEnvironmentStatus {
    DockerEnvironmentStatus {
        state: DockerEnvironmentState::NotInstalled,
        engine_ready: false,
        message: "synthetic unavailable engine".into(),
        desktop_vm: false,
        desktop_transport_verified: false,
        desktop_vm_capacity_verified: false,
        workspace_staging_available: false,
        daemon_version: None,
        architecture: Some("amd64".into()),
        rootless: Some(false),
        cpu_limits: None,
        memory_limits: None,
        pids_limit: None,
        cgroups_supported: None,
        available_images: Vec::new(),
        disk_available_bytes: None,
        endpoint_fingerprint: None,
        daemon_identity_fingerprint: None,
    }
}

fn git_probe(
    executable_state: EvidenceState,
    executable_code: EvidenceCode,
    identity_state: EvidenceState,
    identity_code: EvidenceCode,
) -> GitProbeV1 {
    GitProbeV1 {
        executable_state,
        executable_code,
        identity_state,
        identity_code,
    }
}

#[test]
fn git_prerequisite_is_typed_and_only_positive_probe_resolves_its_task() {
    let missing = catalog::assess_for_scan_with_git(
        &[],
        None,
        &unavailable_docker(),
        HostKind::Linux,
        200,
        11,
        git_probe(
            EvidenceState::Missing,
            EvidenceCode::GitMissing,
            EvidenceState::Blocked,
            EvidenceCode::ParentPrerequisiteUnavailable,
        ),
    );
    let git = missing
        .probe_evidence
        .iter()
        .find(|evidence| evidence.prerequisite.domain == PrerequisiteDomain::Git)
        .unwrap();
    assert_eq!(git.state, EvidenceState::Missing);
    assert!(
        missing
            .findings
            .iter()
            .any(|finding| finding.id == "setup:git:install")
    );
    let identity = missing
        .prerequisite_graph
        .iter()
        .find(|node| node.id.capability == PrerequisiteCapability::GitIdentity)
        .unwrap();
    assert_eq!(
        identity.remediation,
        catalog::RemediationEligibility::BlockedByPrerequisite
    );
    assert_eq!(
        identity.depends_on,
        vec![catalog::PrerequisiteId {
            domain: PrerequisiteDomain::Git,
            platform: HostKind::Linux,
            capability: PrerequisiteCapability::GitExecutable,
            subject: PrerequisiteSubject::None,
        }]
    );
    assert!(!missing.checked_domains.iter().any(|domain| domain == "git"));
    assert!(missing.validate().is_ok());

    let ready = catalog::assess_for_scan_with_git(
        &[],
        None,
        &unavailable_docker(),
        HostKind::Linux,
        201,
        12,
        git_probe(
            EvidenceState::Ready,
            EvidenceCode::GitAvailable,
            EvidenceState::Ready,
            EvidenceCode::GitIdentityConfigured,
        ),
    );
    assert!(ready.checked_domains.iter().any(|domain| domain == "git"));
    assert!(ready.validate().is_ok());
}

#[test]
fn git_executable_alone_does_not_resolve_the_author_identity_task() {
    let snapshot = catalog::assess_for_scan_with_git(
        &[],
        None,
        &unavailable_docker(),
        HostKind::Linux,
        202,
        13,
        git_probe(
            EvidenceState::Ready,
            EvidenceCode::GitAvailable,
            EvidenceState::Missing,
            EvidenceCode::GitIdentityMissing,
        ),
    );
    assert!(
        snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "git:executable")
    );
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "git")
    );
    assert!(
        snapshot
            .findings
            .iter()
            .any(|finding| finding.id == "setup:git:identity")
    );
    assert!(snapshot.validate().is_ok());
}

#[test]
fn git_identity_claim_requires_its_executable_dependency() {
    let mut snapshot = catalog::assess_for_scan_with_git(
        &[],
        None,
        &unavailable_docker(),
        HostKind::Linux,
        203,
        14,
        git_probe(
            EvidenceState::Missing,
            EvidenceCode::GitMissing,
            EvidenceState::Ready,
            EvidenceCode::GitIdentityConfigured,
        ),
    );
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "git:identity")
    );
    assert!(snapshot.validate().is_ok());

    snapshot.checked_domains.push("git:identity".into());
    assert!(snapshot.validate().is_err());
}
