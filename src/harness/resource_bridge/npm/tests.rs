use super::lockfile::{
    LockedPackage, cache_covers_lockfile, collect_lockfiles, decode_base64, npm_cache_digest_path,
    verify_sha512_file,
};
use sha2::{Digest, Sha512};
use std::fs;

mod custom_registry;

#[test]
fn reads_registry_entries_and_skips_local_workspace_entries() {
    let root = std::env::temp_dir().join(format!("koolade-npm-lock-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("packages/app")).unwrap();
    fs::write(
        root.join("package-lock.json"),
        r#"{"lockfileVersion":3,"packages":{"node_modules/pkg":{"resolved":"https://registry.npmjs.org/pkg/-/pkg-1.0.0.tgz","integrity":"sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="},"packages/local":{"resolved":"file:../local"}}}"#,
    )
    .unwrap();
    fs::write(
        root.join("packages/app/package-lock.json"),
        r#"{"packages":{"node_modules/linked":{"resolved":"link:../linked"}}}"#,
    )
    .unwrap();
    let packages = collect_lockfiles(&root).unwrap();
    assert_eq!(
        packages,
        vec![LockedPackage {
            url: "https://registry.npmjs.org/pkg/-/pkg-1.0.0.tgz".into(),
            integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==".into(),
        }]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn package_identity_skips_relative_npm_workspace_links() {
    let identities = super::packages_from_bytes(
        br#"{"lockfileVersion":3,"packages":{"node_modules/local-helper":{"resolved":"fixtures/local-helper","link":true}}}"#,
    )
    .unwrap();
    assert!(identities.is_empty());
}

#[test]
fn npm_workspace_links_cannot_hide_remote_lockfile_urls() {
    let bytes = br#"{"packages":{"node_modules/local-helper":{"resolved":"HTTPS://registry.npmjs.org/pkg.tgz","link":true}}}"#;
    assert!(super::packages_from_bytes(bytes).is_err());

    let root =
        std::env::temp_dir().join(format!("koolade-npm-remote-link-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package-lock.json"), bytes).unwrap();
    assert!(collect_lockfiles(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_lockfile_errors_do_not_disclose_resolved_credentials() {
    let root = std::env::temp_dir().join(format!(
        "koolade-npm-lock-private-url-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let resolved = "https://synthetic-user:synthetic-pass@registry.example.net/pkg.tgz?token=synthetic-lock-token";
    let lockfile = root.join("package-lock.json");
    fs::write(
        &lockfile,
        serde_json::to_vec(&serde_json::json!({
            "packages": { "node_modules/pkg": { "resolved": resolved } }
        }))
        .unwrap(),
    )
    .unwrap();
    let missing_digest = collect_lockfiles(&root).unwrap_err().to_string();
    assert!(!missing_digest.contains("synthetic-user"));
    assert!(!missing_digest.contains("synthetic-pass"));
    assert!(!missing_digest.contains("synthetic-lock-token"));

    fs::write(
        &lockfile,
        serde_json::to_vec(&serde_json::json!({
            "packages": {
                "node_modules/first": { "resolved": resolved, "integrity": "sha512-one" },
                "node_modules/second": { "resolved": resolved, "integrity": "sha512-two" }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let conflict = collect_lockfiles(&root).unwrap_err().to_string();
    assert!(!conflict.contains("synthetic-user"));
    assert!(!conflict.contains("synthetic-pass"));
    assert!(!conflict.contains("synthetic-lock-token"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_lockfiles_are_allowed_for_a_new_npm_project_dependency() {
    let root = std::env::temp_dir().join(format!("koolade-npm-no-lock-{}", uuid::Uuid::new_v4()));
    let response_dir = root.join("resources");
    let cache = root.join("npm-cache");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&response_dir).unwrap();
    fs::create_dir_all(&cache).unwrap();
    assert!(
        super::lockfile::collect_lockfiles_if_present(&root)
            .unwrap()
            .is_empty()
    );
    assert!(collect_lockfiles(&root).is_err());
    assert!(
        super::addition::prepare_existing_lockfile(
            &root,
            &response_dir,
            &cache,
            &std::sync::atomic::AtomicUsize::new(0),
        )
        .unwrap()
        .is_none()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sha512_sri_maps_to_cacache_path_and_verifies_archive_bytes() {
    let directory = std::env::temp_dir().join(format!("koolade-npm-sri-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&directory).unwrap();
    let archive = directory.join("package.tgz");
    fs::write(&archive, b"locked package archive").unwrap();
    let digest = Sha512::digest(b"locked package archive");
    let encoded = base64(&digest);
    let integrity = format!("sha512-{encoded}");
    assert_eq!(decode_base64(&encoded).unwrap(), digest.as_slice());
    assert_eq!(
        npm_cache_digest_path(&directory, &integrity).unwrap(),
        directory
            .join("_cacache/content-v2/sha512")
            .join(hex(&digest[..1]))
            .join(hex(&digest[1..2]))
            .join(hex(&digest[2..]))
    );
    assert!(verify_sha512_file(&archive, &integrity));
    fs::write(&archive, b"tampered archive").unwrap();
    assert!(!verify_sha512_file(&archive, &integrity));
    assert!(decode_base64("not-a-sha512-digest").is_none());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn prepared_cache_is_selected_only_when_it_covers_every_locked_archive() {
    let root = std::env::temp_dir().join(format!("koolade-npm-coverage-{}", uuid::Uuid::new_v4()));
    let cache = root.join("cache");
    fs::create_dir_all(&cache).unwrap();
    let package = b"locked package archive";
    let digest = Sha512::digest(package);
    let integrity = format!("sha512-{}", base64(&digest));
    fs::write(
        root.join("package-lock.json"),
        format!(
            r#"{{"packages":{{"node_modules/pkg":{{"resolved":"https://registry.npmjs.org/pkg/-/pkg-1.0.0.tgz","integrity":"{integrity}"}}}}}}"#
        ),
    )
    .unwrap();
    let content = npm_cache_digest_path(&cache, &integrity).unwrap();
    fs::create_dir_all(content.parent().unwrap()).unwrap();
    fs::write(&content, package).unwrap();
    assert!(cache_covers_lockfile(&root, &cache));
    assert!(super::verified_offline_cache(&root, &cache));
    fs::write(&content, b"tampered cache archive").unwrap();
    assert!(!super::verified_offline_cache(&root, &cache));
    fs::remove_file(content).unwrap();
    assert!(!super::verified_offline_cache(&root, &cache));
    assert!(!cache_covers_lockfile(&root, &cache));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn new_npm_addition_reuses_existing_lockfile_artifacts_before_resolving_the_new_package() {
    let root = std::env::temp_dir().join(format!(
        "koolade-npm-addition-cache-{}",
        uuid::Uuid::new_v4()
    ));
    let worktree = root.join("worktree");
    let response_dir = root.join("resources");
    let cache = root.join("npm-cache");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::create_dir_all(&response_dir).unwrap();
    std::fs::create_dir_all(cache.join("_cacache")).unwrap();
    let archive = b"existing package archive";
    let digest = Sha512::digest(archive);
    let integrity = format!("sha512-{}", base64(&digest));
    std::fs::write(
        worktree.join("package-lock.json"),
        format!(
            r#"{{"packages":{{"node_modules/existing":{{"resolved":"https://registry.npmjs.org/existing/-/existing-1.0.0.tgz","integrity":"{integrity}"}}}}}}"#
        ),
    )
    .unwrap();
    let cached_archive = npm_cache_digest_path(&cache, &integrity).unwrap();
    std::fs::create_dir_all(cached_archive.parent().unwrap()).unwrap();
    std::fs::write(&cached_archive, archive).unwrap();

    let prepared = super::addition::prepare_existing_lockfile(
        &worktree,
        &response_dir,
        &cache,
        &std::sync::atomic::AtomicUsize::new(0),
    )
    .unwrap()
    .expect("existing lockfile should be prepared before adding a package");
    assert_eq!(prepared.status, "prepared");
    assert!(prepared.summary.contains("1 packages reused"));
    let telemetry = prepared
        .preparation
        .expect("cache reuse should be reported structurally");
    assert_eq!(
        telemetry.status,
        Some(crate::harness::DependencyPreparationStatus::AlreadyAvailable)
    );
    assert_eq!(telemetry.package_count, 1);
    assert_eq!(telemetry.cache_hits, 1);
    assert_eq!(telemetry.packages_downloaded, 0);
    assert_eq!(telemetry.bytes_downloaded, 0);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn new_npm_addition_stops_when_existing_manifest_dependencies_have_no_lockfile() {
    let root = std::env::temp_dir().join(format!(
        "koolade-npm-unlocked-addition-{}",
        uuid::Uuid::new_v4()
    ));
    let worktree = root.join("worktree");
    let response_dir = root.join("resources");
    let cache = root.join("npm-cache");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&response_dir).unwrap();
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        worktree.join("package.json"),
        r#"{"dependencies":{"existing-package":"^1.0.0"}}"#,
    )
    .unwrap();

    let response = super::addition::prepare_existing_lockfile(
        &worktree,
        &response_dir,
        &cache,
        &std::sync::atomic::AtomicUsize::new(0),
    )
    .unwrap()
    .expect("unlocked existing dependencies must stop an offline addition retry");
    assert_eq!(response.status, "needs_attention");
    assert!(response.summary.contains("package-lock.json"));
    assert!(response.summary.contains("no offline retry ran"));
    fs::remove_dir_all(root).unwrap();
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(a >> 2) as usize] as char);
        out.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
