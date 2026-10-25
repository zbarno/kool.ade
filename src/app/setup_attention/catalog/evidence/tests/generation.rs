use super::*;

#[test]
fn stale_provider_generation_is_not_promoted_and_unavailable_parent_blocks_images() {
    let mut probe = provider("codex");
    probe.run_generation = 10;
    let scan = super::super::super::assess_for_scan(
        &[probe],
        Some("codex"),
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        200,
        11,
    );
    assert!(scan.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.capability == PrerequisiteCapability::ProviderAuthentication
            && evidence.state == EvidenceState::Stale
    }));
    assert!(
        !scan
            .checked_domains
            .iter()
            .any(|domain| domain == "cli:codex")
    );
    assert!(
        !scan
            .checked_domains
            .iter()
            .any(|domain| domain == "cli:select")
    );
    assert!(scan.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.domain == PrerequisiteDomain::Image
            && evidence.state == EvidenceState::Blocked
    }));
    assert!(scan.prerequisite_graph.iter().any(|node| {
        node.id.domain == PrerequisiteDomain::Image
            && node.remediation
                == super::super::super::RemediationEligibility::BlockedByPrerequisite
    }));
    assert!(scan.validate().is_ok());

    let mut contradictory = scan.clone();
    contradictory.checked_domains.push("cli:codex".into());
    assert!(contradictory.validate().is_err());
}

#[test]
fn trusted_engine_is_required_before_image_absence_is_evidence() {
    let unavailable = super::super::super::assess_for_scan(
        &[],
        None,
        &docker(DockerEnvironmentState::UntrustedEndpoint, false),
        HostKind::Linux,
        200,
        11,
    );
    assert!(!unavailable.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.domain == PrerequisiteDomain::Image
            && evidence.state == EvidenceState::Missing
    }));
    let queryable = super::super::super::assess_for_scan(
        &[],
        None,
        &docker(DockerEnvironmentState::MissingApprovedImage, true),
        HostKind::Linux,
        200,
        11,
    );
    assert!(queryable.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.domain == PrerequisiteDomain::Image
            && evidence.state == EvidenceState::Missing
    }));
}

#[test]
fn checked_domains_reject_unproven_or_unknown_positive_claims() {
    let snapshot = super::super::super::assess_for_scan(
        &[],
        None,
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        250,
        12,
    );
    for domain in [
        "docker",
        "git",
        "git:executable",
        "cli:codex",
        "images:general:amd64",
        "unknown:domain",
    ] {
        let mut contradictory = snapshot.clone();
        contradictory.checked_domains.push(domain.into());
        assert!(contradictory.validate().is_err(), "accepted {domain}");
    }
}
