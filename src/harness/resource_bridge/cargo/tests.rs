use super::{locate_cargo_in_path, run_fetch};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::{fs::PermissionsExt, fs::symlink},
    path::PathBuf,
    sync::atomic::AtomicUsize,
};

#[test]
fn empty_cargo_lock_is_reported_as_already_available() {
    let root =
        std::env::temp_dir().join(format!("koolade-empty-cargo-lock-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("Cargo.lock"), "version = 3\n").unwrap();

    let response = super::prepare(&root, &root.join("cache"), &AtomicUsize::new(0)).unwrap();

    assert_eq!(response.status, "prepared");
    assert_eq!(
        response.preparation.unwrap().status,
        Some(crate::harness::DependencyPreparationStatus::AlreadyAvailable)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn private_configuration_allows_only_verified_cargo_cache_hits() {
    let root = std::env::temp_dir().join(format!(
        "koolade-private-cargo-cache-{}",
        uuid::Uuid::new_v4()
    ));
    let worktree = root.join("worktree");
    let cache = root.join("cargo-cache");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(cache.join("registry/cache/index.crates.io-synthetic")).unwrap();
    let archive = b"verified cached Cargo package";
    let checksum = Sha256::digest(archive)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fs::write(
        worktree.join("Cargo.lock"),
        format!(
            "version = 4\n\n[[package]]\nname = \"synthetic-helper\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{checksum}\"\n"
        ),
    )
    .unwrap();

    let downloaded = AtomicUsize::new(0);
    let missing =
        super::prepare_with_download_policy(&worktree, &cache, &downloaded, false).unwrap();
    assert_eq!(missing.status, "needs_attention");
    assert_eq!(downloaded.load(std::sync::atomic::Ordering::Relaxed), 0);

    fs::write(
        cache.join("registry/cache/index.crates.io-synthetic/synthetic-helper-1.0.0.crate"),
        archive,
    )
    .unwrap();
    let cached =
        super::prepare_with_download_policy(&worktree, &cache, &downloaded, false).unwrap();
    assert_eq!(cached.status, "prepared");
    let telemetry = cached.preparation.unwrap();
    assert_eq!(telemetry.cache_hits, 1);
    assert_eq!(telemetry.bytes_downloaded, 0);
    assert_eq!(downloaded.load(std::sync::atomic::Ordering::Relaxed), 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cargo_proxy_symlink_keeps_its_alias_when_invoked_for_fetch() {
    let root = std::env::temp_dir().join(format!(
        "koolade-cargo-proxy-alias-{}",
        uuid::Uuid::new_v4()
    ));
    let worktree = root.join("worktree");
    let home = root.join("home");
    let bin = home.join(".cargo/bin");
    let cache = root.join("cache");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&cache).unwrap();
    let rustup = bin.join("rustup");
    let cargo = bin.join("cargo");
    fs::write(
        &rustup,
        "#!/bin/sh\nprintf '%s\\n%s\\n' \"$0\" \"$*\" > \"$CARGO_HOME/invocation\"\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(&rustup, fs::Permissions::from_mode(0o755)).unwrap();
    symlink(&rustup, &cargo).unwrap();

    let path = std::env::join_paths([bin.as_path()]).unwrap();
    let located =
        locate_cargo_in_path(&worktree, &path, Some(&home.canonicalize().unwrap())).unwrap();
    assert_eq!(located, cargo);
    assert_ne!(located, rustup.canonicalize().unwrap());

    run_fetch(&located, &worktree, &cache, "http://127.0.0.1:1").unwrap();
    let invocation = fs::read_to_string(cache.join("invocation")).unwrap();
    let mut lines = invocation.lines();
    assert_eq!(PathBuf::from(lines.next().unwrap()), cargo);
    assert!(lines.next().unwrap().contains("--locked fetch"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_lock_identity_with_conflicting_checksums_is_rejected() {
    let lockfile = format!(
        "version = 4\n\n[[package]]\nname = \"same-crate\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{}\"\n\n[[package]]\nname = \"same-crate\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{}\"\n",
        "a".repeat(64),
        "b".repeat(64)
    );

    let error = super::package_identities_from_contents(&lockfile).unwrap_err();

    assert!(error.to_string().contains("conflicting checksums"));
}

#[test]
fn conflicting_registry_cache_copies_fail_closed_regardless_of_scan_order() {
    let root = std::env::temp_dir().join(format!(
        "koolade-cargo-cache-collision-{}",
        uuid::Uuid::new_v4()
    ));
    let first = root.join("registry/cache/index.crates.io-first");
    let second = root.join("registry/cache/index.crates.io-second");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    let valid_archive = b"valid lockfile package";
    let checksum = Sha256::digest(valid_archive)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fs::write(first.join("same-crate-1.0.0.crate"), valid_archive).unwrap();
    fs::write(
        second.join("same-crate-1.0.0.crate"),
        b"colliding package with different bytes",
    )
    .unwrap();

    let package = super::lockfile::LockedPackage {
        name: "same-crate".into(),
        version: "1.0.0".into(),
        checksum,
    };
    let error = super::cache::verified_package_count(&root, &[package]).unwrap_err();

    assert!(error.to_string().contains("did not match its Cargo.lock"));
    fs::remove_dir_all(root).unwrap();
}
