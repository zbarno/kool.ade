use super::*;
use std::sync::atomic::Ordering;

const MOD: &str =
    "module example.invalid/service\n\ngo 1.23\n\nrequire github.com/google/uuid v1.6.0\n";
const SUM: &str = "github.com/google/uuid v1.6.0 h1:NIvaJDMOsjHA8n1jAhLSgzrAzy1Hgr+hNrb57e+94F0=\ngithub.com/google/uuid v1.6.0/go.mod h1:TIyPZe4MgqvfeYDBFedMoGGpEw/LqOeaOT+nhxU+yHo=\n";

#[test]
fn verified_go_snapshot_is_reused_before_authorized_downloads() {
    let worktree = tempfile::tempdir().unwrap();
    std::fs::write(worktree.path().join("go.mod"), MOD).unwrap();
    std::fs::write(worktree.path().join("go.sum"), SUM).unwrap();
    let cache_root = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let lock = lockfile::collect(worktree.path()).unwrap();
    let identity = lockfile::identity(&lock).unwrap();
    let snapshot = cache::snapshot_root(cache_root.path(), &identity).unwrap();
    std::fs::create_dir_all(snapshot.join("pkg/mod")).unwrap();
    let operations = test_preparation_operations_with_verifier(
        |_, _, _| panic!("a verified snapshot must be reused without fetching"),
        |_, _, _| Ok(()),
    );
    let downloaded = AtomicUsize::new(0);

    let response = prepare_with_download_policy(
        worktree.path(),
        cache_root.path(),
        staging.path(),
        &downloaded,
        true,
        Some(&operations),
        &identity,
    )
    .unwrap();

    let telemetry = response
        .preparation
        .as_ref()
        .expect("preparation telemetry");
    assert_eq!(
        telemetry.status,
        Some(crate::harness::DependencyPreparationStatus::AlreadyAvailable)
    );
    assert_eq!(telemetry.package_count, lock.packages.len() as u64);
    assert_eq!(telemetry.cache_hits, telemetry.package_count);
    assert_eq!(telemetry.packages_downloaded, 0);
    assert_eq!(telemetry.bytes_downloaded, 0);
    assert_eq!(downloaded.load(Ordering::Relaxed), 0);
}

#[test]
fn changed_go_lock_identity_cannot_use_a_verified_snapshot() {
    let worktree = tempfile::tempdir().unwrap();
    std::fs::write(worktree.path().join("go.mod"), MOD).unwrap();
    std::fs::write(worktree.path().join("go.sum"), SUM).unwrap();
    let cache_root = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let lock = lockfile::collect(worktree.path()).unwrap();
    let identity = lockfile::identity(&lock).unwrap();
    let snapshot = cache::snapshot_root(cache_root.path(), &identity).unwrap();
    std::fs::create_dir_all(snapshot.join("pkg/mod")).unwrap();
    let operations = test_preparation_operations_with_verifier(
        |_, _, _| panic!("changed lock inputs must fail before fetching"),
        |worktree, _, _| {
            std::fs::write(worktree.join("go.mod"), MOD.replace("go 1.23", "go 1.24"))?;
            Ok(())
        },
    );

    let error = prepare_with_download_policy(
        worktree.path(),
        cache_root.path(),
        staging.path(),
        &AtomicUsize::new(0),
        true,
        Some(&operations),
        &identity,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Go task dependency inputs changed while preparing")
    );
}
