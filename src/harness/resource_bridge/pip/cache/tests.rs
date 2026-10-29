use super::super::lockfile::LockedPackage;
use super::{snapshot_root, verified_wheel_count, verify_wheelhouse};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};

fn package(bytes: &[u8]) -> LockedPackage {
    LockedPackage {
        name: "sample-package".into(),
        version: "1.2.3".into(),
        hashes: BTreeSet::from([hex(&Sha256::digest(bytes))]),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn verifies_only_hash_matching_universal_wheels_and_lock_scoped_snapshots() {
    let root = std::env::temp_dir().join(format!("koolade-pip-cache-{}", uuid::Uuid::new_v4()));
    let identity = format!("sha256:{}", "a".repeat(64));
    let identity_dir = root.join("sha256").join("a".repeat(64));
    let wheels = identity_dir.join("wheels");
    fs::create_dir_all(&wheels).unwrap();
    let content = b"synthetic wheel bytes";
    fs::write(
        wheels.join("sample_package-1.2.3-py3-none-any.whl"),
        content,
    )
    .unwrap();
    let expected = package(content);

    assert_eq!(
        verify_wheelhouse(&wheels, std::slice::from_ref(&expected)).unwrap(),
        1
    );
    assert_eq!(
        verified_wheel_count(&root, &identity, &[expected]).unwrap(),
        1
    );
    assert_eq!(snapshot_root(&root, &identity).unwrap(), identity_dir);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_platform_wheels_source_archives_and_unlocked_packages() {
    let root = std::env::temp_dir().join(format!("koolade-pip-wheel-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let content = b"synthetic wheel bytes";
    let expected = package(content);
    for name in [
        "sample_package-1.2.3-cp311-cp311-manylinux_2_28_x86_64.whl",
        "sample_package-1.2.3.tar.gz",
        "other_package-1.2.3-py3-none-any.whl",
    ] {
        let path = root.join(name);
        fs::write(&path, content).unwrap();
        assert!(
            verify_wheelhouse(&root, std::slice::from_ref(&expected)).is_err(),
            "accepted {name}"
        );
        fs::remove_file(path).unwrap();
    }
    fs::write(
        root.join("sample_package-1.2.3-py3-none-any.whl"),
        b"tampered",
    )
    .unwrap();
    assert!(verify_wheelhouse(&root, &[expected]).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_invalid_lockfile_cache_identities_and_symlinked_identity_roots() {
    let root = std::env::temp_dir().join(format!("koolade-pip-root-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    assert!(snapshot_root(&root, "sha256:bad").is_err());
    let outside = root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let sha_root = root.join("sha256");
    fs::create_dir_all(&sha_root).unwrap();
    std::os::unix::fs::symlink(&outside, sha_root.join("b".repeat(64))).unwrap();
    assert!(verified_wheel_count(&root, &format!("sha256:{}", "b".repeat(64)), &[]).is_err());
    fs::remove_dir_all(root).unwrap();
}
