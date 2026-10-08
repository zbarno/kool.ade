use super::base64;
use crate::harness::resource_bridge::{ResourceResponse, SANDBOX_RESOURCE_DIR};
use sha2::{Digest, Sha512};
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

#[test]
fn authorized_addition_prepares_a_custom_registry_archive_from_verified_cache() {
    let root = std::env::temp_dir().join(format!(
        "koolade-npm-custom-addition-{}",
        uuid::Uuid::new_v4()
    ));
    let worktree = root.join("worktree");
    let response_dir = root.join("resources");
    let cache = root.join("npm-cache");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&response_dir).unwrap();
    fs::create_dir_all(&cache).unwrap();

    let need = crate::harness::DependencyNeed {
        ecosystem: crate::harness::PackageEcosystem::Npm,
        package: Some("zod".into()),
        version: Some("1.0.0".into()),
        source: Some("https://packages.example.net/".into()),
        command: "npm install zod@1.0.0 --registry=https://packages.example.net/".into(),
        reason: "Validate imported project settings".into(),
        kind: crate::harness::DependencyKind::NewProjectDependency,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    };
    assert!(crate::harness::dependency_decision_allowed(
        &need,
        crate::harness::DependencyDecision::UserAuthorizeForTask
    ));

    let archive = b"verified custom registry package";
    let integrity = format!("sha512-{}", base64(&Sha512::digest(archive)));
    let cached_archive = super::super::lockfile::npm_cache_digest_path(&cache, &integrity).unwrap();
    fs::create_dir_all(cached_archive.parent().unwrap()).unwrap();
    fs::write(cached_archive, archive).unwrap();

    let response = super::super::addition::prepare_with_resolver(
        super::super::addition::AdditionRequest {
            worktree: &worktree,
            response_dir: &response_dir,
            npm_cache: &cache,
            npm_snapshot: &response_dir.join("npm-index-snapshots"),
            need: &need,
            decision: crate::harness::DependencyDecision::UserAuthorizeForTask,
            downloaded_bytes: &AtomicUsize::new(0),
        },
        |project, spec, registry| {
            assert_eq!(spec, "zod@1.0.0");
            assert_eq!(registry, "https://packages.example.net/");
            fs::write(
                project.join("package-lock.json"),
                serde_json::to_vec(&serde_json::json!({
                    "lockfileVersion": 3,
                    "packages": {
                        "node_modules/zod": {
                            "version": "1.0.0",
                            "resolved": "https://packages.example.net/zod/-/zod-1.0.0.tgz",
                            "integrity": integrity,
                        }
                    }
                }))?,
            )?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(response.status, "prepared");
    assert!(response.summary.contains("1 packages reused"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn npm_addition_checks_the_exact_manager_or_user_authorization_scope() {
    let root = std::env::temp_dir().join(format!(
        "koolade-npm-addition-scopes-{}",
        uuid::Uuid::new_v4()
    ));
    let worktree = root.join("worktree");
    let response_dir = root.join("resources");
    let cache = root.join("npm-cache");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&response_dir).unwrap();
    fs::create_dir_all(&cache).unwrap();

    let archive = b"verified public npm package";
    let integrity = format!("sha512-{}", base64(&Sha512::digest(archive)));
    let cached_archive = super::super::lockfile::npm_cache_digest_path(&cache, &integrity).unwrap();
    fs::create_dir_all(cached_archive.parent().unwrap()).unwrap();
    fs::write(&cached_archive, archive).unwrap();
    let need = crate::harness::DependencyNeed {
        ecosystem: crate::harness::PackageEcosystem::Npm,
        package: Some("zod".into()),
        version: Some("^1.0.0".into()),
        source: Some("https://registry.npmjs.org/".into()),
        command: "npm install zod@^1.0.0".into(),
        reason: "Add the package required by the assigned feature".into(),
        kind: crate::harness::DependencyKind::NewProjectDependency,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    };

    for decision in [
        crate::harness::DependencyDecision::AutoAuthorize,
        crate::harness::DependencyDecision::AuthorizeForTask,
        crate::harness::DependencyDecision::AuthorizeForProject,
        crate::harness::DependencyDecision::UserAuthorizeForTask,
        crate::harness::DependencyDecision::UserAuthorizeForProject,
    ] {
        let response = super::super::addition::prepare_with_resolver(
            super::super::addition::AdditionRequest {
                worktree: &worktree,
                response_dir: &response_dir,
                npm_cache: &cache,
                npm_snapshot: &response_dir.join("npm-index-snapshots"),
                need: &need,
                decision,
                downloaded_bytes: &AtomicUsize::new(0),
            },
            |project, spec, registry| {
                assert_eq!(spec, "zod@^1.0.0");
                assert_eq!(registry, "https://registry.npmjs.org/");
                fs::write(
                    project.join("package-lock.json"),
                    serde_json::to_vec(&serde_json::json!({
                        "lockfileVersion": 3,
                        "packages": {
                            "node_modules/zod": {
                                "version": "1.0.0",
                                "resolved": "https://registry.npmjs.org/zod/-/zod-1.0.0.tgz",
                                "integrity": integrity,
                            }
                        }
                    }))?,
                )?;
                Ok(())
            },
        )
        .unwrap_or_else(|error| panic!("decision {decision:?} should be accepted: {error:#}"));
        assert_eq!(response.status, "prepared", "{decision:?}");
    }

    let rejected = super::super::addition::prepare_with_resolver(
        super::super::addition::AdditionRequest {
            worktree: &worktree,
            response_dir: &response_dir,
            npm_cache: &cache,
            npm_snapshot: &response_dir.join("npm-index-snapshots"),
            need: &need,
            decision: crate::harness::DependencyDecision::RequiresUserAuthorization,
            downloaded_bytes: &AtomicUsize::new(0),
        },
        |_, _, _| panic!("unapproved npm addition must not resolve"),
    );
    assert!(rejected.is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn authorized_custom_registry_prepares_a_cold_cache_archive_through_the_registry_fetch_path() {
    let root = std::env::temp_dir().join(format!(
        "koolade-npm-custom-cold-cache-{}",
        uuid::Uuid::new_v4()
    ));
    let worktree = root.join("worktree");
    let response_dir = root.join("resources");
    let cache = root.join("npm-cache");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&response_dir).unwrap();
    fs::create_dir_all(&cache).unwrap();

    let archive = b"cold custom registry package archive";
    let integrity = format!("sha512-{}", base64(&Sha512::digest(archive)));
    fs::write(
        worktree.join("package-lock.json"),
        serde_json::to_vec(&serde_json::json!({
            "lockfileVersion": 3,
            "packages": {
                "node_modules/zod": {
                    "version": "1.0.0",
                    "resolved": "https://packages.example.net/zod/-/zod-1.0.0.tgz",
                    "integrity": integrity,
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(!super::super::verified_offline_cache(&worktree, &cache));

    let registry = url::Url::parse("https://packages.example.net/").unwrap();
    let downloaded = AtomicUsize::new(0);
    let fetch_count = AtomicUsize::new(0);
    let response = super::super::prepare_with_registry_and_ops(
        super::super::PreparationRequest {
            worktree: &worktree,
            response_dir: &response_dir,
            npm_cache: &cache,
            npm_snapshot: &response_dir.join("npm-index-snapshots"),
            purpose: "Prepare an authorized custom npm dependency",
            downloaded_bytes: &downloaded,
            authorized_registry: Some(&registry),
        },
        super::super::PreparationOperations {
            retrieve: &|resource_dir, url, _, remaining, authorized_registry| {
                fetch_count.fetch_add(1, Ordering::Relaxed);
                assert_eq!(url, "https://packages.example.net/zod/-/zod-1.0.0.tgz");
                assert_eq!(authorized_registry, Some(&registry));
                assert!(remaining >= archive.len() as u64);
                fs::write(resource_dir.join("downloaded.tgz"), archive)?;
                Ok(ResourceResponse::allowed_file(
                    "verified custom registry archive".into(),
                    format!("{SANDBOX_RESOURCE_DIR}/downloaded.tgz"),
                    archive.len(),
                ))
            },
            index_cache: &|_, npm_cache, archives, _| {
                assert_eq!(archives.len(), 1);
                let cached = super::super::lockfile::npm_cache_digest_path(npm_cache, &integrity)
                    .expect("lockfile SHA-512 maps into the cache");
                fs::create_dir_all(cached.parent().unwrap())?;
                fs::copy(&archives[0], cached)?;
                Ok(())
            },
        },
    )
    .unwrap();

    assert_eq!(fetch_count.load(Ordering::Relaxed), 1);
    assert_eq!(downloaded.load(Ordering::Relaxed), archive.len());
    assert_eq!(response.status, "prepared");
    assert!(
        response
            .summary
            .contains("1 lockfile-pinned packages retrieved")
    );
    assert!(super::super::verified_offline_cache(&worktree, &cache));
    fs::remove_dir_all(root).unwrap();
}
