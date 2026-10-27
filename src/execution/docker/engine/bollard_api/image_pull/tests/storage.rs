use std::sync::mpsc;

use super::super::{is_storage_exhaustion_hint, verify_pull_storage_capacity};
use crate::execution::docker::{
    discovery::PrerequisiteCode,
    engine::{EngineSnapshot, ImagePullControl},
    images::{DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES, ImageLock, RuntimeProfile},
};

fn desktop_snapshot() -> EngineSnapshot {
    EngineSnapshot {
        daemon_id: "synthetic-desktop".into(),
        daemon_version: "28.0.0".into(),
        api_version: "1.53".into(),
        minimum_api_version: "1.41".into(),
        os_type: "linux".into(),
        architecture: "amd64".into(),
        security_options: vec!["name=seccomp".into()],
        rootless: false,
        native_linux: false,
        docker_desktop: true,
        memory_limit: true,
        cpu_quota: true,
        pids_limit: true,
        cgroup_version: Some("2".into()),
        cgroup_driver: Some("systemd".into()),
        seccomp_enabled: true,
        docker_storage_accessible: false,
        disk_available_bytes: None,
        minimum_disk_bytes: 1024,
    }
}

#[test]
fn only_the_bounded_general_image_may_bootstrap_unknown_desktop_capacity() {
    let snapshot = desktop_snapshot();
    let lock = ImageLock::bundled().unwrap();
    let general = lock.select(RuntimeProfile::General, "amd64").unwrap();
    let rust = lock.select(RuntimeProfile::Rust, "amd64").unwrap();

    let (progress, _events) = mpsc::sync_channel(1);
    let control = ImagePullControl::new(progress);
    assert_eq!(
        verify_pull_storage_capacity(true, &general, &snapshot, None).unwrap(),
        Some(DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES)
    );
    control.restrict_reported_bytes(DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES);
    let over_limit = i64::try_from(DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES + 1).unwrap();
    assert_eq!(
        control
            .record_progress(Some("oversized-layer"), Some(over_limit), Some(over_limit))
            .unwrap_err()
            .code,
        PrerequisiteCode::ImageUnavailable
    );
    assert_eq!(
        verify_pull_storage_capacity(true, &rust, &snapshot, None)
            .unwrap_err()
            .code,
        PrerequisiteCode::InsufficientDisk
    );
}

#[test]
fn verified_desktop_capacity_is_used_for_later_profiles_and_storage_bounds() {
    let snapshot = desktop_snapshot();
    let rust = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::Rust, "amd64")
        .unwrap();

    assert_eq!(
        verify_pull_storage_capacity(true, &rust, &snapshot, Some(8 * 1024 * 1024 * 1024)).unwrap(),
        None
    );
    assert_eq!(
        verify_pull_storage_capacity(true, &rust, &snapshot, Some(1024))
            .unwrap_err()
            .code,
        PrerequisiteCode::InsufficientDisk
    );
    assert_eq!(
        verify_pull_storage_capacity(true, &rust, &snapshot, None)
            .unwrap_err()
            .code,
        PrerequisiteCode::InsufficientDisk
    );
}

#[test]
fn enospc_pull_errors_are_redacted_and_non_retryable() {
    let detail = "no space left on device at /private/path";
    assert!(is_storage_exhaustion_hint(detail));
    assert!(is_storage_exhaustion_hint("disk quota exceeded"));
    assert!(!is_storage_exhaustion_hint(
        "registry temporarily unavailable"
    ));
    let failure = super::super::storage_exhausted();
    assert_eq!(failure.code, PrerequisiteCode::InsufficientDisk);
    assert!(!failure.message.contains("/private/path"));
}
