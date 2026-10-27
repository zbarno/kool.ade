use super::*;

fn status(state: State) -> DockerEnvironmentStatus {
    DockerEnvironmentStatus {
        state,
        engine_ready: false,
        message: String::new(),
        desktop_vm: false,
        desktop_transport_verified: false,
        desktop_vm_capacity_verified: false,
        workspace_staging_available: false,
        daemon_version: None,
        architecture: None,
        rootless: None,
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

#[test]
fn windows_install_task_uses_the_official_publisher_and_explicit_permissions() {
    let finding = finding(&status(State::NotInstalled), HostKind::Windows).unwrap();
    assert_eq!(finding.id, "setup:docker:install");
    assert!(finding.title.contains("Docker Desktop for Windows"));
    assert!(finding.next_step.contains("official Docker website"));
    assert!(finding.next_step.contains("publisher is Docker, Inc."));
    assert!(
        finding
            .next_step
            .contains("Do not use an unofficial installer")
    );
}

#[test]
fn windows_first_run_and_linux_backend_are_separate_from_installation() {
    let first_run = finding(&status(State::NotRunning), HostKind::Windows).unwrap();
    assert_eq!(first_run.id, "setup:docker:start");
    assert!(first_run.next_step.contains("WSL 2 based engine"));
    assert!(first_run.next_step.contains("permission prompts"));

    let wrong_container_mode =
        finding(&status(State::WindowsContainersRequired), HostKind::Windows).unwrap();
    assert_eq!(wrong_container_mode.id, "setup:docker:windows-containers");
    assert!(
        wrong_container_mode
            .next_step
            .contains("switch to Linux containers")
    );
}

#[test]
fn windows_pipe_and_endpoint_recovery_never_suggest_acl_or_remote_fallbacks() {
    let permission = finding(&status(State::PermissionDenied), HostKind::Windows).unwrap();
    assert!(
        permission
            .next_step
            .contains("Do not change the named-pipe ACL")
    );
    assert!(permission.next_step.contains("contact your administrator"));

    let endpoint = finding(&status(State::UntrustedEndpoint), HostKind::Windows).unwrap();
    assert!(endpoint.next_step.contains("Remote, TCP, SSH"));
}

#[test]
fn windows_engine_architecture_and_vm_storage_remain_honestly_gated() {
    let architecture = finding(&status(State::UnsupportedArchitecture), HostKind::Windows).unwrap();
    assert_eq!(architecture.status, super::super::FindingStatus::Unverified);
    assert!(
        architecture
            .explanation
            .contains("outside the approved runtime image matrix")
    );
    assert!(
        architecture
            .next_step
            .contains("Native Windows ARM64 support has not been validated")
    );

    let disk = finding(&status(State::InsufficientDisk), HostKind::Windows).unwrap();
    assert!(disk.title.contains("Linux VM"));
    assert!(
        disk.next_step
            .contains("Windows host free space does not prove VM capacity")
    );
}

#[test]
fn other_platforms_keep_their_existing_generic_guidance() {
    let linux = finding(&status(State::NotInstalled), HostKind::Linux).unwrap();
    assert_eq!(linux.title, "Configure secure Docker");
}
