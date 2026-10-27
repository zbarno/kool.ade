use super::probe::endpoint_fingerprint;
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
        desktop_transport_verified: false,
        desktop_vm_capacity_verified: false,
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
        desktop_transport_verified: false,
        desktop_vm_capacity_verified: false,
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

#[test]
fn desktop_execution_readiness_requires_verified_transport_and_engine_storage() {
    let mut status = DockerEnvironmentStatus::from_readiness(EngineReadiness {
        ready: true,
        desktop_transport_verified: true,
        desktop_vm_capacity_verified: true,
        daemon_identity_fingerprint: Some("verified-desktop-daemon".into()),
        docker_desktop: true,
        daemon_version: Some("28.0.0".into()),
        api_version: Some("1.53".into()),
        architecture: Some("amd64".into()),
        rootless: Some(false),
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

    assert!(status.desktop_transport_verified);
    assert_eq!(
        status.local_execution_ready(),
        cfg!(any(target_os = "macos", target_os = "windows"))
    );

    status.desktop_transport_verified = false;
    assert!(!status.local_execution_ready());
}

#[cfg(all(target_os = "linux", unix))]
#[test]
fn test_only_rootless_endpoints_skip_persistent_canonical_socket_selection() {
    use std::os::unix::net::UnixListener;

    let temporary = tempfile::tempdir().unwrap();
    let home = std::fs::canonicalize(temporary.path()).unwrap();
    let action_dir = home.join("setup-docker-action");
    let run_dir = action_dir.join("run-test");
    std::fs::create_dir_all(&run_dir).unwrap();
    let socket = run_dir.join("docker.sock");
    let _listener = UnixListener::bind(&socket).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("HOME", &home);
    environment.set("KOOLADE_HOME", temporary.path().join("koolade-home"));
    environment.set("DOCKER_HOST", format!("unix://{}", socket.display()));
    environment.set("KOOLADE_TEST_ROOTLESS_ACTION_SOCKET", &socket);
    environment.remove("DOCKER_CONTEXT");
    environment.remove("KOOLADE_DOCKER_DAEMON_RESTART_INTEGRATION");

    let mut status = DockerEnvironmentStatus::from_readiness(EngineReadiness {
        ready: true,
        daemon_identity_fingerprint: Some("a".repeat(64)),
        docker_desktop: false,
        desktop_transport_verified: false,
        desktop_vm_capacity_verified: false,
        daemon_version: Some("29.0.0".into()),
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
    super::rootless_selection::persist_rootless_selection(&mut status);

    assert!(status.engine_ready);
    assert_eq!(status.state, DockerEnvironmentState::MissingApprovedImage);
    assert!(
        !crate::persistence::linux_rootless_docker_selection::settings_path().exists(),
        "test-only action socket must not write the operator's persistent Linux selection"
    );

    environment.remove("KOOLADE_TEST_ROOTLESS_ACTION_SOCKET");
    environment.set("KOOLADE_DOCKER_DAEMON_RESTART_INTEGRATION", "1");
    let daemon_socket = std::env::temp_dir().join(format!(
        "koolade-docker-{}.sock",
        uuid::Uuid::new_v4().simple()
    ));
    environment.set("DOCKER_HOST", format!("unix://{}", daemon_socket.display()));
    let mut daemon_status = status.clone();
    super::rootless_selection::persist_rootless_selection(&mut daemon_status);

    assert!(daemon_status.engine_ready);
    assert_eq!(
        daemon_status.state,
        DockerEnvironmentState::MissingApprovedImage
    );
    assert!(
        !crate::persistence::linux_rootless_docker_selection::settings_path().exists(),
        "test-only daemon socket must not write the operator's persistent Linux selection"
    );
}
