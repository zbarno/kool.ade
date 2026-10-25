use super::*;
use crate::execution::docker::environment::{DockerEnvironmentState, DockerEnvironmentStatus};

fn ready_engine() -> crate::execution::docker::environment::DockerEnvironmentStatus {
    DockerEnvironmentStatus {
        state: DockerEnvironmentState::MissingApprovedImage,
        engine_ready: true,
        message: "synthetic ready engine".into(),
        desktop_vm: false,
        daemon_version: Some("28.0.0".into()),
        architecture: Some("amd64".into()),
        rootless: Some(true),
        cpu_limits: Some(true),
        memory_limits: Some(true),
        pids_limit: Some(true),
        cgroups_supported: Some(true),
        available_images: Vec::new(),
        disk_available_bytes: Some(1_000_000_000),
        endpoint_fingerprint: Some("synthetic-endpoint".into()),
        daemon_identity_fingerprint: Some("synthetic-daemon".into()),
    }
}

fn snapshot(
    git: catalog::EvidenceState,
    code: catalog::EvidenceCode,
) -> catalog::MachineSnapshotV1 {
    catalog::assess_for_scan_with_git(
        &[],
        None,
        &ready_engine(),
        catalog::HostKind::Linux,
        100,
        1,
        catalog::GitProbeV1 {
            executable_state: git,
            executable_code: code,
            identity_state: catalog::EvidenceState::Blocked,
            identity_code: catalog::EvidenceCode::ParentPrerequisiteUnavailable,
        },
    )
}

#[test]
fn missing_general_image_or_git_cannot_produce_a_ready_result() {
    let general_image = snapshot(
        catalog::EvidenceState::Ready,
        catalog::EvidenceCode::GitAvailable,
    );
    assert!(
        general_image
            .findings
            .iter()
            .any(|finding| finding.id.starts_with("setup:images:general:"))
    );
    assert_eq!(
        issue_for_readiness(None, true, &general_image).unwrap().id,
        "setup:bootstrap"
    );

    let git_missing = snapshot(
        catalog::EvidenceState::Missing,
        catalog::EvidenceCode::GitMissing,
    );
    assert_eq!(
        issue_for_readiness(None, true, &git_missing).unwrap().id,
        "setup:bootstrap"
    );
}

/// Production boundary: the final worker check compares the daemon identity and
/// selected provider before committing the local task snapshot. This pure test
/// exercises the identity predicate without Docker, a network request, or disk
/// access; setup cost is negligible.
#[test]
fn daemon_or_provider_switch_rejects_the_scan_identity() {
    let docker = ready_engine();
    let result = crate::app::setup_attention::SetupScanResult {
        generation: 3,
        issue: None,
        snapshot: None,
        endpoint_fingerprint: Some("synthetic-endpoint".into()),
        daemon_identity_fingerprint: Some("synthetic-daemon".into()),
        docker_readiness_fingerprint: Some(docker.readiness_fingerprint()),
        selected_provider: None,
        cancelled: false,
    };
    let matches =
        |result: &crate::app::setup_attention::SetupScanResult,
         selected,
         before,
         after,
         docker: &crate::execution::docker::environment::DockerEnvironmentStatus| {
            super::verification_identity_matches(result, selected, before, after, docker)
        };
    assert!(matches(
        &result,
        None,
        Some("synthetic-endpoint".into()),
        Some("synthetic-endpoint".into()),
        &docker,
    ));
    assert!(!matches(
        &result,
        Some(catalog::ProviderIdV1::Codex),
        Some("synthetic-endpoint".into()),
        Some("synthetic-endpoint".into()),
        &docker,
    ));
    let mut changed_readiness = docker.clone();
    changed_readiness.cgroups_supported = Some(false);
    assert!(!matches(
        &result,
        None,
        Some("synthetic-endpoint".into()),
        Some("synthetic-endpoint".into()),
        &changed_readiness,
    ));
    let mut switched_daemon = docker;
    switched_daemon.daemon_identity_fingerprint = Some("different-daemon".into());
    assert!(!matches(
        &result,
        None,
        Some("synthetic-endpoint".into()),
        Some("synthetic-endpoint".into()),
        &switched_daemon,
    ));
}
