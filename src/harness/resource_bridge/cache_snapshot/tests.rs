use super::*;
use crate::execution::contract::DependencyCacheEcosystemV1;
use std::fs;

fn capture(root: &std::path::Path, ecosystem: DependencyCacheEcosystemV1) -> CacheSnapshot {
    CacheSnapshot::capture(
        root,
        "00000000-0000-4000-8000-000000000001",
        ecosystem,
        "00000000-0000-4000-8000-000000000002",
        "sha256:resolved",
        "x86_64",
        "00000000-0000-4000-8000-000000000003",
    )
    .unwrap()
}

#[test]
fn captures_deterministic_verified_snapshot_without_host_paths() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("registry/cache")).unwrap();
    fs::write(
        root.path().join("registry/cache/pkg.crate"),
        b"package bytes",
    )
    .unwrap();
    let snapshot = capture(root.path(), DependencyCacheEcosystemV1::Cargo);
    snapshot
        .manifest
        .validate_ref(&snapshot.cache_ref, &snapshot.archive)
        .unwrap();
    assert!(
        !serde_json::to_string(&snapshot.manifest)
            .unwrap()
            .contains(root.path().to_str().unwrap())
    );
    let repeated = capture(root.path(), DependencyCacheEcosystemV1::Cargo);
    assert_eq!(snapshot.archive, repeated.archive);
    assert_eq!(snapshot.manifest, repeated.manifest);
    assert_eq!(snapshot.cache_ref, repeated.cache_ref);
}

#[test]
fn rejects_symlinks_and_archive_tampering() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("pkg"), b"package bytes").unwrap();
    let snapshot = capture(root.path(), DependencyCacheEcosystemV1::Npm);
    let mut corrupt = snapshot.archive.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(
        snapshot
            .manifest
            .validate_ref(&snapshot.cache_ref, &corrupt)
            .is_err()
    );
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("entry"), b"outside cache bytes").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("nested")).unwrap();
        assert!(
            CacheSnapshot::capture(
                root.path(),
                "00000000-0000-4000-8000-000000000001",
                DependencyCacheEcosystemV1::Npm,
                "00000000-0000-4000-8000-000000000002",
                "sha256:resolved",
                "x86_64",
                "local"
            )
            .is_err()
        );
    }
}
