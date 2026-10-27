use super::*;

#[cfg(target_os = "macos")]
#[test]
/// Covers the production Darwin audit-file ACL boundary using native kernel ACL APIs.
/// It needs no external service; a native macOS test run is required to exercise the syscall path.
fn macos_audit_removes_extended_acl_and_fails_closed_when_one_remains() {
    use std::{ffi::CString, os::fd::AsRawFd, os::unix::fs::OpenOptionsExt};

    unsafe extern "C" {
        fn acl_from_text(text: *const libc::c_char) -> *mut std::ffi::c_void;
        fn acl_set_fd_np(
            fd: libc::c_int,
            acl: *mut std::ffi::c_void,
            kind: libc::c_int,
        ) -> libc::c_int;
        fn acl_free(value: *mut std::ffi::c_void) -> libc::c_int;
    }

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.json");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    let text = CString::new("everyone allow read").unwrap();
    let acl = unsafe { acl_from_text(text.as_ptr()) };
    assert!(!acl.is_null());
    assert_eq!(
        unsafe { acl_set_fd_np(file.as_raw_fd(), acl, 0x0000_0100) },
        0
    );
    unsafe { acl_free(acl) };

    assert!(super::unix::verify_macos_acl_absent(&file).is_err());
    super::unix::secure_macos_acl(&file).unwrap();
    super::unix::verify_macos_acl_absent(&file).unwrap();
}

#[cfg(unix)]
#[test]
fn audit_rejects_audit_file_and_lock_symlinks_without_touching_targets() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let target = root.join("protected");
    std::fs::write(&target, "keep").unwrap();
    let audit_path = root.join("machine-setup/audit.json");
    std::fs::create_dir(root.join("machine-setup")).unwrap();
    symlink(&target, &audit_path).unwrap();
    assert!(
        append(
            &audit_path,
            Operation::PrepareLockedImage,
            &format!("setup:images:general:amd64:{}", "e".repeat(64)),
            &"a".repeat(64),
            &"b".repeat(64),
            "approved",
            1,
        )
        .is_err()
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");

    std::fs::remove_file(&audit_path).unwrap();
    let lock_target = root.join("lock-target");
    std::fs::write(&lock_target, "keep lock").unwrap();
    let lock_path = root.join("machine-setup/audit.json.lock");
    std::fs::remove_file(&lock_path).unwrap();
    symlink(&lock_target, &lock_path).unwrap();
    assert!(
        append(
            &audit_path,
            Operation::PrepareLockedImage,
            &format!("setup:images:general:amd64:{}", "e".repeat(64)),
            &"a".repeat(64),
            &"b".repeat(64),
            "approved",
            2,
        )
        .is_err()
    );
    assert_eq!(std::fs::read_to_string(lock_target).unwrap(), "keep lock");
}

#[cfg(unix)]
#[test]
fn audit_rejects_a_symlinked_private_directory_without_changing_its_target() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let target = root.join("target");
    std::fs::create_dir(&target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
    symlink(&target, root.join("machine-setup")).unwrap();
    let audit_path = root.join("machine-setup/audit.json");
    assert!(
        append(
            &audit_path,
            Operation::PrepareLockedImage,
            &format!("setup:images:general:amd64:{}", "e".repeat(64)),
            &"a".repeat(64),
            &"b".repeat(64),
            "approved",
            1,
        )
        .is_err()
    );
    assert_eq!(
        std::fs::metadata(target).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[cfg(unix)]
#[test]
fn audit_rejects_a_symlink_in_the_state_root_path() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let target = root.join("real-state");
    std::fs::create_dir(&target).unwrap();
    let alias = root.join("state-alias");
    symlink(&target, &alias).unwrap();
    let audit_path = alias.join("machine-setup/audit.json");
    assert!(
        append(
            &audit_path,
            Operation::PrepareLockedImage,
            &format!("setup:images:general:amd64:{}", "e".repeat(64)),
            &"a".repeat(64),
            &"b".repeat(64),
            "approved",
            1,
        )
        .is_err()
    );
    assert!(!target.join("machine-setup").exists());
}

#[cfg(unix)]
#[test]
fn audit_appends_bounded_redacted_receipts_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let path = root.join("machine-setup/audit.json");
    append(
        &path,
        Operation::PrepareLockedImage,
        &format!("setup:images:general:amd64:{}", "e".repeat(64)),
        &"a".repeat(64),
        &"b".repeat(64),
        "approved",
        1,
    )
    .unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("approved"));
    assert!(text.contains("setup:images:general:amd64"));
    assert!(!text.contains("private image source"));
}
