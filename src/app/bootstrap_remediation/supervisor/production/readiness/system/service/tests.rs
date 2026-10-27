use super::unit::{self, APP_UNIT_NAME, UnitFileState, UnitProperties};
use super::{command_line_is_rootless, path_absent};
use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
};

const TEMPLATE: &[u8] = b"[Unit]\nDescription=Kool.ad/e rootless Docker service\n\n[Service]\nEnvironment=PATH=/usr/bin:/sbin:/usr/sbin:/bin\nExecStart=/usr/bin/dockerd-rootless.sh\nType=notify\nDelegate=yes\nRestart=always\n\n[Install]\nWantedBy=default.target\n";

fn state(load: &str, active: &str, fragment: &str) -> UnitProperties {
    UnitProperties {
        load_state: load.into(),
        active_state: active.into(),
        fragment_path: fragment.into(),
    }
}

#[test]
fn exact_inactive_or_failed_app_unit_can_resume_but_active_or_replaced_cannot() {
    let path =
        std::path::Path::new("/home/test/.config/systemd/user/koolade-rootless-docker.service");
    assert!(unit::retryable(
        &state("loaded", "inactive", path.to_str().unwrap()),
        true,
        path
    ));
    assert!(unit::retryable(
        &state("loaded", "failed", path.to_str().unwrap()),
        true,
        path
    ));
    assert!(unit::retryable(
        &state("not-found", "inactive", ""),
        true,
        path
    ));
    assert!(!unit::retryable(
        &state("loaded", "active", path.to_str().unwrap()),
        true,
        path
    ));
    assert!(!unit::retryable(
        &state("loaded", "inactive", "/tmp/other.service"),
        true,
        path
    ));
    assert!(!unit::retryable(
        &state("loaded", "inactive", path.to_str().unwrap()),
        false,
        path
    ));
}

#[test]
fn an_absent_unit_is_retryable_only_when_systemd_confirms_not_found_and_inactive() {
    let path =
        std::path::Path::new("/home/test/.config/systemd/user/koolade-rootless-docker.service");
    assert!(unit::retryable(
        &state("not-found", "inactive", ""),
        false,
        path
    ));
    assert!(!unit::retryable(
        &state("loaded", "inactive", path.to_str().unwrap()),
        false,
        path
    ));
    assert!(!unit::retryable(
        &state("not-found", "failed", ""),
        false,
        path
    ));
}

#[test]
fn exact_app_template_is_read_only_and_conflicts_remain_untouched() {
    let uid = unsafe { libc::geteuid() };
    let parent = tempfile::tempdir().unwrap();
    let path = parent.path().join(APP_UNIT_NAME);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    file.write_all(TEMPLATE).unwrap();
    drop(file);

    let file_state = unit::inspect_app_unit(&path, uid);
    assert_eq!(file_state, UnitFileState::Exact);
    assert!(unit::resumable_app_unit(
        file_state,
        &state("loaded", "failed", path.to_str().unwrap()),
        &path,
    ));
    assert!(!unit::resumable_app_unit(
        file_state,
        &state("loaded", "active", path.to_str().unwrap()),
        &path,
    ));
    assert_eq!(
        unit::inspect_app_unit(&path, uid.wrapping_add(1)),
        UnitFileState::Conflict
    );
    let manual = b"[Service]\nExecStart=/bin/custom-dockerd\n";
    std::fs::write(&path, manual).unwrap();
    let conflict = unit::inspect_app_unit(&path, uid);
    assert_eq!(conflict, UnitFileState::Conflict);
    assert!(!unit::resumable_app_unit(
        conflict,
        &state("loaded", "inactive", path.to_str().unwrap()),
        &path,
    ));
    assert_eq!(std::fs::read(&path).unwrap(), manual);
}

#[test]
fn symlink_and_group_writable_app_units_are_rejected() {
    let uid = unsafe { libc::geteuid() };
    let parent = tempfile::tempdir().unwrap();
    let path = parent.path().join(APP_UNIT_NAME);
    let outside = parent.path().join("manual-unit");
    std::fs::write(&outside, TEMPLATE).unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert_eq!(unit::inspect_app_unit(&path, uid), UnitFileState::Conflict);
    std::fs::remove_file(&path).unwrap();

    std::fs::write(&path, TEMPLATE).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o620)).unwrap();
    assert_eq!(unit::inspect_app_unit(&path, uid), UnitFileState::Conflict);
}

#[test]
fn systemd_properties_must_be_complete_and_unique() {
    let parsed = unit::parse_properties(
        "LoadState=loaded\nActiveState=failed\nFragmentPath=/home/test/service\n",
    )
    .unwrap();
    assert_eq!(parsed.load_state, "loaded");
    assert_eq!(parsed.active_state, "failed");
    assert_eq!(parsed.fragment_path, "/home/test/service");
    assert!(unit::parse_properties("LoadState=loaded\nActiveState=inactive\n").is_none());
    assert!(
        unit::parse_properties(
            "LoadState=loaded\nLoadState=loaded\nActiveState=inactive\nFragmentPath=x\n"
        )
        .is_none()
    );
}

#[test]
fn other_loaded_rootless_units_stay_conflicts_and_rootful_service_is_not_rootless() {
    let path =
        std::path::Path::new("/home/test/.config/systemd/user/koolade-rootless-docker.service");
    let loaded_other_unit = state("loaded", "inactive", path.to_str().unwrap());
    assert!(!unit::retryable(&loaded_other_unit, false, path));
    assert!(!command_line_is_rootless(
        b"/usr/bin/dockerd --host=unix:///run/docker.sock\0"
    ));
    assert!(command_line_is_rootless(b"/usr/bin/dockerd-rootless.sh\0"));
    assert!(command_line_is_rootless(b"rootlesskit\0dockerd\0"));
}

#[test]
fn any_existing_or_unsafe_socket_path_is_rejected_without_changes() {
    let parent = tempfile::tempdir().unwrap();
    let socket = parent.path().join("docker.sock");
    assert!(path_absent(&socket).ready());
    std::fs::write(&socket, b"manual socket placeholder").unwrap();
    assert!(!path_absent(&socket).ready());
    assert_eq!(
        std::fs::read(&socket).unwrap(),
        b"manual socket placeholder"
    );
    std::fs::remove_file(&socket).unwrap();
    std::os::unix::fs::symlink("missing-target", &socket).unwrap();
    assert!(!path_absent(&socket).ready());
}
