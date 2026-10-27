use std::os::unix::fs::PermissionsExt;

use super::open_trusted_home;

fn current_uid() -> Option<u32> {
    let uid = unsafe { libc::geteuid() };
    (uid != 0).then_some(uid)
}

#[test]
fn accepts_a_home_under_root_owned_sticky_tmp() {
    let Some(uid) = current_uid() else { return };
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    open_trusted_home(home.path(), uid)
        .expect("temporary home should satisfy the trusted path contract");
}

#[test]
fn rejects_a_replaceable_group_or_world_writable_ancestor() {
    let Some(uid) = current_uid() else { return };
    let base = tempfile::tempdir().unwrap();
    std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let shared = base.path().join("shared");
    let home = shared.join("home");
    std::fs::create_dir(&shared).unwrap();
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o770)).unwrap();
    std::fs::create_dir(&home).unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert!(open_trusted_home(&home, uid).is_err());
}

#[test]
fn rejects_a_home_path_for_a_different_account() {
    let Some(uid) = current_uid() else { return };
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();

    assert!(open_trusted_home(home.path(), uid.wrapping_add(1)).is_err());
}
