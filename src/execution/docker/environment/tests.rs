use super::*;

fn failure(code: PrerequisiteCode) -> PrerequisiteFailure {
    PrerequisiteFailure {
        code,
        message: "probe diagnostic".into(),
        retryable: false,
    }
}

#[test]
fn context_switch_changes_only_the_ephemeral_endpoint_fingerprint() {
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let default = endpoint_fingerprint(&endpoint, Some("default"), None);
    let desktop = endpoint_fingerprint(&endpoint, Some("desktop-linux"), None);
    assert_ne!(default, desktop);
    assert!(!default.contains("docker_engine"));
    assert!(!desktop.contains("desktop-linux"));
}

#[test]
fn readiness_failures_map_to_actionable_settings_states() {
    for (code, expected) in [
        (
            PrerequisiteCode::DockerMissing,
            DockerEnvironmentState::NotInstalled,
        ),
        (
            PrerequisiteCode::DaemonStopped,
            DockerEnvironmentState::NotRunning,
        ),
        (
            PrerequisiteCode::PermissionDenied,
            DockerEnvironmentState::PermissionDenied,
        ),
        (
            PrerequisiteCode::RootfulLinux,
            DockerEnvironmentState::RootfulLinuxRejected,
        ),
        (
            PrerequisiteCode::ImageUnavailable,
            DockerEnvironmentState::ApprovedImageUncheckable,
        ),
        (
            PrerequisiteCode::CgroupsUnavailable,
            DockerEnvironmentState::CgroupsUnavailable,
        ),
        (
            PrerequisiteCode::LimitsUnavailable,
            DockerEnvironmentState::LimitsUnavailable,
        ),
        (
            PrerequisiteCode::UntrustedEndpoint,
            DockerEnvironmentState::UntrustedEndpoint,
        ),
    ] {
        assert_eq!(
            DockerEnvironmentStatus::from_failure(failure(code), false).state,
            expected
        );
    }
    assert_eq!(
        DockerEnvironmentStatus::from_failure(failure(PrerequisiteCode::DockerMissing), true).state,
        DockerEnvironmentState::DesktopVmRequired
    );
}

#[test]
fn docker_failure_categories_remain_distinct_for_readiness_evidence() {
    for (code, expected) in [
        (
            PrerequisiteCode::WindowsContainers,
            DockerEnvironmentState::WindowsContainersRequired,
        ),
        (
            PrerequisiteCode::InsufficientDisk,
            DockerEnvironmentState::InsufficientDisk,
        ),
        (
            PrerequisiteCode::LimitsUnavailable,
            DockerEnvironmentState::LimitsUnavailable,
        ),
        (
            PrerequisiteCode::SeccompUnavailable,
            DockerEnvironmentState::SeccompUnavailable,
        ),
        (
            PrerequisiteCode::UnsupportedApi,
            DockerEnvironmentState::UnsupportedApi,
        ),
        (
            PrerequisiteCode::UnsupportedOs,
            DockerEnvironmentState::UnsupportedOs,
        ),
        (
            PrerequisiteCode::UnsupportedArchitecture,
            DockerEnvironmentState::UnsupportedArchitecture,
        ),
        (
            PrerequisiteCode::WorkspaceUnavailable,
            DockerEnvironmentState::WorkspaceUnavailable,
        ),
        (
            PrerequisiteCode::ProfileUnsupported,
            DockerEnvironmentState::ProfileUnsupported,
        ),
        (
            PrerequisiteCode::CgroupsUnavailable,
            DockerEnvironmentState::CgroupsUnavailable,
        ),
    ] {
        let status = DockerEnvironmentStatus::from_failure(failure(code), false);
        assert_eq!(status.state, expected, "{code:?}");
    }
}

#[test]
fn missing_approved_image_is_reported_without_disabling_engine_capabilities() {
    let status = DockerEnvironmentStatus::from_readiness(EngineReadiness {
        ready: true,
        daemon_identity_fingerprint: Some("daemon-fingerprint".into()),
        docker_desktop: false,
        daemon_version: Some("28.0.0".into()),
        api_version: Some("1.53".into()),
        architecture: Some("amd64".into()),
        rootless: Some(true),
        linux_containers: Some(true),
        seccomp_enabled: Some(true),
        memory_limit: Some(true),
        cpu_quota: Some(true),
        pids_limit: Some(true),
        cgroups_supported: Some(true),
        workspace_staging_available: true,
        disk_available_bytes: Some(1_000_000_000),
        available_images: Vec::new(),
        failure: None,
    });
    assert_eq!(status.state, DockerEnvironmentState::MissingApprovedImage);
    assert!(status.ready());
    assert_eq!(
        status.daemon_identity_fingerprint.as_deref(),
        Some("daemon-fingerprint")
    );
    let encoded = serde_json::to_string(&status).unwrap();
    assert!(!encoded.contains("daemon-fingerprint"));
}

#[test]
fn docker_desktop_engine_health_does_not_imply_local_execution_support() {
    let mut status = DockerEnvironmentStatus::from_readiness(EngineReadiness {
        ready: true,
        daemon_identity_fingerprint: Some("synthetic-daemon".into()),
        docker_desktop: true,
        daemon_version: Some("28.0.0".into()),
        api_version: Some("1.53".into()),
        architecture: Some("amd64".into()),
        rootless: Some(true),
        linux_containers: Some(true),
        seccomp_enabled: Some(true),
        memory_limit: Some(true),
        cpu_quota: Some(true),
        pids_limit: Some(true),
        cgroups_supported: Some(true),
        workspace_staging_available: true,
        disk_available_bytes: Some(1_000_000_000),
        available_images: vec!["approved-image".into()],
        failure: None,
    });

    assert!(status.ready());
    status.desktop_vm = true;
    assert!(!status.local_execution_ready());
}
