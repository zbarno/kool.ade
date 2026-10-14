use super::*;

#[test]
fn rootful_linux_is_rejected_even_if_container_uid_is_non_root() {
    let mut engine = snapshot();
    engine.rootless = false;
    engine.security_options = vec!["name=seccomp,profile=builtin".into()];
    assert_eq!(
        validate_snapshot(&engine).unwrap_err().code,
        PrerequisiteCode::RootfulLinux
    );
}

#[test]
fn windows_containers_are_rejected_before_other_checks() {
    let mut engine = snapshot();
    engine.os_type = "windows".into();
    assert_eq!(
        validate_snapshot(&engine).unwrap_err().code,
        PrerequisiteCode::WindowsContainers
    );
}

#[test]
fn api_range_and_resource_limit_checks_fail_closed() {
    let mut engine = snapshot();
    engine.minimum_api_version = "not-an-api".into();
    assert_eq!(
        validate_snapshot(&engine).unwrap_err().code,
        PrerequisiteCode::UnsupportedApi
    );
    engine = snapshot();
    engine.minimum_api_version = "1.54".into();
    assert_eq!(
        validate_snapshot(&engine).unwrap_err().code,
        PrerequisiteCode::UnsupportedApi
    );
    engine = snapshot();
    engine.memory_limit = false;
    assert_eq!(
        validate_snapshot(&engine).unwrap_err().code,
        PrerequisiteCode::LimitsUnavailable
    );
    engine = snapshot();
    engine.seccomp_enabled = false;
    assert_eq!(
        validate_snapshot(&engine).unwrap_err().code,
        PrerequisiteCode::SeccompUnavailable
    );
}

#[test]
fn unconfined_daemon_security_profile_is_rejected() {
    let mut engine = snapshot();
    engine
        .security_options
        .push("name=seccomp,profile=unconfined".into());
    assert_eq!(
        validate_snapshot(&engine).unwrap_err().code,
        PrerequisiteCode::SeccompUnavailable
    );
    engine.security_options.pop();
    engine
        .security_options
        .push("name=apparmor,profile=unconfined".into());
    assert_eq!(
        validate_snapshot(&engine).unwrap_err().code,
        PrerequisiteCode::SeccompUnavailable
    );
}

#[test]
fn unsupported_architecture_and_insufficient_disk_are_reported() {
    let mut engine = snapshot();
    engine.architecture = "s390x".into();
    assert_eq!(
        validate_snapshot(&engine).unwrap_err().code,
        PrerequisiteCode::UnsupportedArchitecture
    );
    engine = snapshot();
    engine.disk_available_bytes = Some(1);
    assert_eq!(
        validate_snapshot(&engine).unwrap_err().code,
        PrerequisiteCode::InsufficientDisk
    );
    engine = snapshot();
    engine.docker_storage_accessible = false;
    assert_eq!(
        validate_snapshot(&engine).unwrap_err().code,
        PrerequisiteCode::InsufficientDisk
    );
}

#[test]
fn docker_desktop_never_uses_host_disk_as_vm_storage_evidence() {
    let mut engine = snapshot();
    engine.docker_desktop = true;
    engine.rootless = false;
    engine.docker_storage_accessible = false;
    engine.disk_available_bytes = None;

    let failure = validate_snapshot(&engine).unwrap_err();

    assert_eq!(failure.code, PrerequisiteCode::InsufficientDisk);
    assert!(
        failure
            .message
            .contains("Docker Desktop's VM storage capacity")
    );
}
