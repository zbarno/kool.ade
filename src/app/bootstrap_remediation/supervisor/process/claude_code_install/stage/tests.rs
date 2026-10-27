use super::*;
use sha2::Digest;
use std::os::unix::fs::PermissionsExt;

fn source(directory: &std::path::Path, bytes: &[u8]) -> (std::path::PathBuf, String) {
    let path = directory.join("source");
    std::fs::write(&path, bytes).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    (path, format!("{:x}", sha2::Sha256::digest(bytes)))
}

#[test]
fn atomically_installs_only_the_verified_bytes_and_never_overwrites() {
    let stage = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    std::fs::set_permissions(bin.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (source_path, digest) = source(stage.path(), b"synthetic claude binary");
    let source = std::fs::File::open(source_path).unwrap();
    let directory = std::fs::File::open(bin.path()).unwrap();
    atomic_install_at(&source, &directory, "claude", &digest).unwrap();
    assert_eq!(
        std::fs::read(bin.path().join("claude")).unwrap(),
        b"synthetic claude binary"
    );
    assert!(atomic_install_at(&source, &directory, "claude", &"0".repeat(64)).is_err());
    assert_eq!(
        std::fs::read(bin.path().join("claude")).unwrap(),
        b"synthetic claude binary"
    );
}

#[test]
fn refuses_symlink_and_corrupt_existing_targets() {
    let stage = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    std::fs::set_permissions(bin.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (source_path, digest) = source(stage.path(), b"synthetic claude binary");
    let source = std::fs::File::open(source_path).unwrap();
    let directory = std::fs::File::open(bin.path()).unwrap();
    let other = bin.path().join("other");
    std::fs::write(&other, b"untrusted target").unwrap();
    std::os::unix::fs::symlink(&other, bin.path().join("claude")).unwrap();
    assert!(atomic_install_at(&source, &directory, "claude", &digest).is_err());

    std::fs::remove_file(bin.path().join("claude")).unwrap();
    std::fs::write(bin.path().join("claude"), b"untrusted executable").unwrap();
    std::fs::set_permissions(
        bin.path().join("claude"),
        std::fs::Permissions::from_mode(0o500),
    )
    .unwrap();
    assert!(atomic_install_at(&source, &directory, "claude", &digest).is_err());
    assert_eq!(
        std::fs::read(bin.path().join("claude")).unwrap(),
        b"untrusted executable"
    );
}

#[test]
fn creates_only_current_user_owned_local_directories_and_rejects_redirects() {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let bin = open_user_bin(home.path()).unwrap();
    assert_eq!(bin.metadata().unwrap().mode() & 0o777, 0o700);
    assert!(home.path().join(".local/bin").is_dir());

    let redirected_home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(
        redirected_home.path(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let external_local = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(external_local.path(), redirected_home.path().join(".local"))
        .unwrap();
    assert!(open_user_bin(redirected_home.path()).is_err());
}

#[test]
fn staging_directory_must_be_private_and_unredirected() {
    let staging = tempfile::tempdir().unwrap();
    std::fs::set_permissions(staging.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    validate_private_staging(staging.path()).unwrap();
    std::fs::set_permissions(staging.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(validate_private_staging(staging.path()).is_err());
}
