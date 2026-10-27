use super::*;
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
};

fn test_uid() -> Option<u32> {
    let uid = unsafe { libc::geteuid() };
    (uid != 0).then_some(uid)
}

fn unit_path(home: &Path) -> PathBuf {
    home.join(".config/systemd/user/koolade-rootless-docker.service")
}

fn private_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    home
}

fn prepare_unit_directory(home: &Path, uid: u32) -> File {
    let home = open_home_directory(home, uid).unwrap();
    let config = open_or_create_directory(&home, c".config", uid).unwrap();
    let systemd = open_or_create_directory(&config, c"systemd", uid).unwrap();
    open_or_create_directory(&systemd, c"user", uid).unwrap()
}

#[test]
fn creates_only_fixed_unit_with_private_modes_and_is_idempotent() {
    let Some(uid) = test_uid() else { return };
    let home = private_home();
    let path = unit_path(home.path());

    assert_eq!(
        ensure_unit_for_uid(home.path(), uid).unwrap(),
        UnitFileOutcome::Created
    );
    assert_eq!(std::fs::read(&path).unwrap(), UNIT_TEMPLATE);
    let file_metadata = std::fs::symlink_metadata(&path).unwrap();
    assert_eq!(file_metadata.uid(), uid);
    assert_eq!(file_metadata.mode() & 0o777, 0o600);

    for directory in [
        home.path().join(".config"),
        home.path().join(".config/systemd"),
        home.path().join(".config/systemd/user"),
    ] {
        let metadata = std::fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir());
        assert_eq!(metadata.uid(), uid);
        assert_eq!(metadata.mode() & 0o022, 0);
    }
    assert_eq!(
        ensure_unit_for_uid(home.path(), uid).unwrap(),
        UnitFileOutcome::AlreadyPresent
    );
}

#[test]
fn accepts_an_existing_exact_template_but_rejects_conflicting_contents() {
    let Some(uid) = test_uid() else { return };
    let home = private_home();
    let directory = prepare_unit_directory(home.path(), uid);
    std::fs::write(unit_path(home.path()), UNIT_TEMPLATE).unwrap();
    std::fs::set_permissions(
        unit_path(home.path()),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();

    assert_eq!(
        ensure_unit_file(&directory, uid).unwrap(),
        UnitFileOutcome::AlreadyPresent
    );

    std::fs::write(unit_path(home.path()), b"[Service]\nExecStart=/bin/evil\n").unwrap();
    assert!(ensure_unit_file(&directory, uid).is_err());
    assert_eq!(
        std::fs::read(unit_path(home.path())).unwrap(),
        b"[Service]\nExecStart=/bin/evil\n"
    );
}

#[test]
fn refuses_symlinked_home_config_and_unit_paths() {
    let Some(uid) = test_uid() else { return };
    let parent = private_home();
    let home = parent.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
    let elsewhere = parent.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, home.join(".config")).unwrap();
    assert!(ensure_unit_for_uid(&home, uid).is_err());
    assert!(!elsewhere.join("systemd").exists());

    let home = private_home();
    let directory = prepare_unit_directory(home.path(), uid);
    let outside = parent.path().join("outside-unit");
    std::fs::write(&outside, UNIT_TEMPLATE).unwrap();
    std::os::unix::fs::symlink(outside, unit_path(home.path())).unwrap();
    assert!(ensure_unit_file(&directory, uid).is_err());
}

#[test]
fn refuses_group_or_world_writable_directories_and_wrong_home_owner() {
    let Some(uid) = test_uid() else { return };
    let home = private_home();
    let config = home.path().join(".config");
    std::fs::create_dir(&config).unwrap();
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(ensure_unit_for_uid(home.path(), uid).is_err());

    let home = private_home();
    assert!(ensure_unit_for_uid(home.path(), uid.wrapping_add(1)).is_err());
}

#[test]
fn refuses_root_and_non_absolute_or_parent_component_home_paths() {
    assert!(ensure_unit_for_uid(Path::new("/tmp"), 0).is_err());
    let Some(uid) = test_uid() else { return };
    assert!(ensure_unit_for_uid(Path::new("relative/home"), uid).is_err());
    let home = private_home();
    let traversal = home.path().join("../outside");
    assert!(ensure_unit_for_uid(&traversal, uid).is_err());
}
