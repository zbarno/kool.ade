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

#[cfg(target_os = "linux")]
#[test]
fn snapshot_validation_rejects_a_docker_failure_with_a_positive_image_domain() {
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipping positive rootless smoke fixture for uid 0");
        return;
    }
    let mut snapshot = assess_with_current_smoke_receipt(13);
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

#[cfg(target_os = "linux")]
#[test]
fn snapshot_validation_rejects_an_exact_image_domain_with_its_failure() {
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipping positive rootless smoke fixture for uid 0");
        return;
    }
    for image_failure in [true, false] {
        let mut snapshot = assess_with_current_smoke_receipt(16);
        let failure_id = if image_failure {
            let identity = snapshot
                .checked_domains
                .iter()
                .find_map(|domain| domain.strip_prefix("images:general:amd64:"))
                .unwrap();
            format!("setup:images:general:amd64:{identity}")
        } else {
            "setup:docker:image-digest".to_owned()
        };
        snapshot.findings.push(Finding::action(
            &failure_id,
            "Verify the approved runtime image",
            "The runtime image could not be verified.",
            "Review runtime image readiness, then retry.",
        ));

        assert!(snapshot.validate().is_err(), "accepted {failure_id}");
    }
}

#[cfg(target_os = "linux")]
fn assess_with_current_smoke_receipt(checked_at_unix_ms: u64) -> MachineSnapshotV1 {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.remove("DOCKER_HOST");
    environment.remove("DOCKER_CONTEXT");
    environment.remove("DOCKER_CONFIG");
    let _home = crate::test_support::KooladeHomeGuard::set(home.path());
    let state_dir = crate::persistence::state_root().join("machine-setup");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

    let image = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let endpoint = DockerEnvironmentStatus::current_endpoint_identity_fingerprint().unwrap();
    let daemon = "d".repeat(64);
    let version = "29.0.0";
    crate::persistence::linux_rootless_smoke_receipt::record(
        &endpoint,
        &daemon,
        version,
        &image,
        checked_at_unix_ms.saturating_sub(1).max(1),
    )
    .unwrap();

    let mut engine = docker(DockerEnvironmentState::Ready, true);
    engine.daemon_version = Some(version.into());
    engine.available_images.push(image.reference);
    engine.daemon_identity_fingerprint = Some(daemon);
    assess_with_images(
        &engine,
        std::collections::BTreeSet::new(),
        [RuntimeProfile::General].into_iter().collect(),
        checked_at_unix_ms,
    )
}
