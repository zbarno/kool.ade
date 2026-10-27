use super::*;
use crate::execution::docker::environment::DockerEnvironmentState;

fn desktop_status() -> DockerEnvironmentStatus {
    DockerEnvironmentStatus {
        state: DockerEnvironmentState::InsufficientDisk,
        engine_ready: false,
        message: String::new(),
        desktop_vm: true,
        desktop_transport_verified: true,
        desktop_vm_capacity_verified: false,
        workspace_staging_available: true,
        daemon_version: Some("28.0.0".into()),
        architecture: Some("amd64".into()),
        rootless: Some(false),
        cpu_limits: Some(true),
        memory_limits: Some(true),
        pids_limit: Some(true),
        cgroups_supported: Some(true),
        available_images: Vec::new(),
        disk_available_bytes: None,
        endpoint_fingerprint: None,
        daemon_identity_fingerprint: Some("d".repeat(64)),
    }
}

#[test]
fn desktop_capacity_bootstrap_requires_trusted_transport_and_unknown_storage() {
    for host in [HostKind::MacOs, HostKind::Windows] {
        assert!(desktop_capacity_probe_required(
            &desktop_status(),
            host,
            "amd64"
        ));
    }

    let mut status = desktop_status();
    status.desktop_transport_verified = false;
    assert!(!desktop_capacity_probe_required(
        &status,
        HostKind::MacOs,
        "amd64"
    ));

    let mut status = desktop_status();
    status.disk_available_bytes = Some(1_000_000_000);
    assert!(!desktop_capacity_probe_required(
        &status,
        HostKind::Windows,
        "amd64"
    ));

    let mut status = desktop_status();
    status.workspace_staging_available = false;
    assert!(!desktop_capacity_probe_required(
        &status,
        HostKind::Windows,
        "amd64"
    ));

    let mut status = desktop_status();
    status.desktop_vm_capacity_verified = true;
    assert!(!desktop_capacity_probe_required(
        &status,
        HostKind::MacOs,
        "amd64"
    ));

    let mut status = desktop_status();
    status.state = DockerEnvironmentState::CapabilitiesIncompatible;
    assert!(!desktop_capacity_probe_required(
        &status,
        HostKind::MacOs,
        "amd64"
    ));

    assert!(!desktop_capacity_probe_required(
        &desktop_status(),
        HostKind::Linux,
        "amd64"
    ));
}

#[test]
fn capacity_bootstrap_uses_only_supported_general_image_architectures() {
    assert!(bounded_desktop_general_image(
        &ImageLock::bundled().unwrap(),
        "amd64"
    ));
    assert!(bounded_desktop_general_image(
        &ImageLock::bundled().unwrap(),
        "arm64"
    ));
    assert!(!bounded_desktop_general_image(
        &ImageLock::bundled().unwrap(),
        "mips64"
    ));
}
