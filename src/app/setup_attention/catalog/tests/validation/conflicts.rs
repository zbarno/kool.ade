use super::*;

#[test]
fn snapshot_validation_rejects_a_docker_failure_with_a_positive_readiness_domain() {
    let mut snapshot = assess(
        &[],
        None,
        &docker(DockerEnvironmentState::Ready, true),
        HostKind::Linux,
        12,
    );
    snapshot.findings.push(Finding::action(
        "setup:docker:rootless",
        "Configure secure Docker",
        "Docker is using a disallowed daemon mode.",
        "Configure rootless Docker, then retry.",
    ));

    assert!(snapshot.validate().is_err());
}

#[test]
fn snapshot_validation_rejects_a_docker_failure_with_a_positive_image_domain() {
    let image = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let mut engine = docker(DockerEnvironmentState::Ready, true);
    engine.available_images.push(image.reference);
    let mut snapshot = assess_with_images(
        &engine,
        std::collections::BTreeSet::new(),
        [RuntimeProfile::General].into_iter().collect(),
        13,
    );
    assert!(
        snapshot
            .checked_domains
            .iter()
            .any(|domain| domain.starts_with("images:general:amd64:"))
    );
    snapshot.findings.push(Finding::action(
        "setup:docker:rootless",
        "Configure secure Docker",
        "Docker is using a disallowed daemon mode.",
        "Configure rootless Docker, then retry.",
    ));

    assert!(snapshot.validate().is_err());
}

#[test]
fn snapshot_validation_rejects_provider_readiness_with_its_failure() {
    let mut snapshot = assess(
        &[agent(ReadinessState::Ready)],
        Some("codex"),
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        14,
    );
    assert!(
        snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "cli:codex")
    );
    snapshot.findings.push(Finding::action(
        "setup:cli:codex:auth",
        "Connect the selected provider",
        "Provider authentication failed.",
        "Authenticate with the provider, then retry.",
    ));

    assert!(snapshot.validate().is_err());
}

#[test]
fn snapshot_validation_rejects_git_readiness_with_its_failure() {
    let mut snapshot = crate::app::setup_attention::catalog::assess_for_scan_with_git(
        &[],
        None,
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        15,
        1,
        GitProbeV1 {
            executable_state: EvidenceState::Ready,
            executable_code: EvidenceCode::GitAvailable,
            identity_state: EvidenceState::Ready,
            identity_code: EvidenceCode::GitIdentityConfigured,
        },
    );
    assert!(
        snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "git")
    );
    snapshot.findings.push(Finding::action(
        "setup:git:identity",
        "Configure a Git identity",
        "Git identity is unavailable.",
        "Configure Git identity, then retry.",
    ));

    assert!(snapshot.validate().is_err());
}

#[test]
fn snapshot_validation_rejects_an_exact_image_domain_with_its_failure() {
    let engine_identity = docker(DockerEnvironmentState::Ready, true)
        .engine_identity_fingerprint()
        .unwrap();
    let image_failure_id = format!("setup:images:general:amd64:{engine_identity}");
    for failure_id in [image_failure_id.as_str(), "setup:docker:image-digest"] {
        let image = ImageLock::bundled()
            .unwrap()
            .select(RuntimeProfile::General, "amd64")
            .unwrap();
        let mut engine = docker(DockerEnvironmentState::Ready, true);
        engine.available_images.push(image.reference);
        let mut snapshot = assess_with_images(
            &engine,
            std::collections::BTreeSet::new(),
            [RuntimeProfile::General].into_iter().collect(),
            16,
        );
        snapshot.findings.push(Finding::action(
            failure_id,
            "Verify the approved runtime image",
            "The runtime image could not be verified.",
            "Review runtime image readiness, then retry.",
        ));

        assert!(snapshot.validate().is_err(), "accepted {failure_id}");
    }
}
