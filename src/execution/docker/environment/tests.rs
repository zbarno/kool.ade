use super::*;

fn failure(code: PrerequisiteCode) -> PrerequisiteFailure {
    PrerequisiteFailure {
        code,
        message: "probe diagnostic".into(),
        retryable: false,
    }
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
            DockerEnvironmentState::MissingApprovedImage,
        ),
        (
            PrerequisiteCode::LimitsUnavailable,
            DockerEnvironmentState::CapabilitiesIncompatible,
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
fn missing_approved_image_is_reported_without_disabling_engine_capabilities() {
    let status = DockerEnvironmentStatus::from_readiness(EngineReadiness {
        ready: true,
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
        workspace_staging_available: true,
        disk_available_bytes: Some(1_000_000_000),
        available_images: Vec::new(),
        failure: None,
    });
    assert_eq!(status.state, DockerEnvironmentState::MissingApprovedImage);
    assert!(status.ready());
}
