use super::*;
use crate::harness::{
    DependencyDecision, DependencyKind, DependencyNeed, DependencyRequestStatus, PackageEcosystem,
};
use sha2::{Digest, Sha512};
use std::{fs, io::Read, path::Path, process::Command};

#[test]
fn authorized_broker_request_prepares_a_verified_npm_cache_hit() {
    let test_root = std::env::temp_dir().join(format!(
        "koolade-resource-authorized-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&test_root).unwrap();
    let _home = crate::test_support::KooladeHomeGuard::set(&test_root.join("state"));
    let worktree = test_root.join("worktree");
    fs::create_dir_all(&worktree).unwrap();
    let archive = format!("synthetic npm package {}", uuid::Uuid::new_v4()).into_bytes();
    let digest = Sha512::digest(&archive);
    let integrity = format!("sha512-{}", base64(&digest));
    fs::write(
        worktree.join("package-lock.json"),
        serde_json::to_vec(&serde_json::json!({
            "lockfileVersion": 3,
            "packages": {
                "node_modules/zod": {
                    "version": "4.0.0",
                    "resolved": "https://registry.npmjs.org/zod/-/zod-4.0.0.tgz",
                    "integrity": integrity,
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    commit_baseline(&worktree);

    let (bridge, updates) = start_bridge(&worktree, None);
    let archive_path = bridge
        .npm_cache_path()
        .join("_cacache/content-v2/sha512")
        .join(hex(&digest[..1]))
        .join(hex(&digest[1..2]))
        .join(hex(&digest[2..]));
    fs::create_dir_all(archive_path.parent().unwrap()).unwrap();
    fs::write(&archive_path, &archive).unwrap();

    let mut client = UnixStream::connect(bridge.socket_path()).unwrap();
    serde_json::to_writer(
        &mut client,
        &ResourceRequest {
            action: super::super::ResourceAction::DependencyRequest,
            manager: None,
            url: None,
            dependency: Some(DependencyNeed {
                ecosystem: PackageEcosystem::Npm,
                package: None,
                version: None,
                source: Some("https://registry.npmjs.org".into()),
                command: "npm ci".into(),
                reason: "Restore the task project's locked npm dependencies".into(),
                kind: DependencyKind::ExistingRestore,
                lockfile_identity: None,
                introduced_packages: Vec::new(),
            }),
            dependency_request_id: None,
            retry_succeeded: None,
            purpose: "Restore locked npm packages".into(),
        },
    )
    .unwrap();
    client.write_all(b"\n").unwrap();

    let request = take_dependency_update(&updates);
    assert_eq!(request.status, DependencyRequestStatus::ManagerReviewing);
    assert!(request.need.lockfile_identity.is_some());
    assert!(request.need.introduced_packages.is_empty());
    answer(
        &request,
        DependencyDecision::AuthorizeForTask,
        None,
        "The locked public npm restore is required by the task.",
    );

    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    let response: ResourceResponse = serde_json::from_str(&response).unwrap();
    assert_eq!(response.status, "prepared");
    assert!(response.summary.contains("1 packages reused"));
    assert_eq!(
        response
            .dependency_request
            .as_ref()
            .map(|request| request.status),
        Some(DependencyRequestStatus::Prepared)
    );
    assert!(
        response
            .dependency_request
            .as_ref()
            .and_then(|request| request.preparation.as_ref())
            .and_then(|preparation| preparation.broker_duration_millis)
            .is_some()
    );
    let prepared_request = response.dependency_request.as_ref().unwrap();
    let prepared_request_id = prepared_request.id.clone();
    let broker_duration = prepared_request
        .preparation
        .as_ref()
        .and_then(|preparation| preparation.broker_duration_millis)
        .unwrap();
    assert!(updates.try_iter().any(|progress| {
        progress.dependency_requests.iter().any(|request| {
            request.id == prepared_request_id
                && request
                    .preparation
                    .as_ref()
                    .and_then(|preparation| preparation.broker_duration_millis)
                    == Some(broker_duration)
        })
    }));
    let snapshot = bridge
        .cache_snapshot(
            "00000000-0000-4000-8000-000000000001",
            crate::execution::contract::DependencyCacheEcosystemV1::Npm,
            "00000000-0000-4000-8000-000000000002",
            &integrity,
            "amd64",
            "local",
        )
        .unwrap();
    snapshot
        .manifest
        .validate_ref(&snapshot.cache_ref, &snapshot.archive)
        .unwrap();
    assert!(
        snapshot
            .manifest
            .entries
            .iter()
            .any(|entry| entry.path.ends_with(&hex(&digest[2..])))
    );

    drop(bridge);
    fs::remove_file(archive_path).unwrap();
    drop(_home);
    fs::remove_dir_all(test_root).unwrap();
}

#[test]
fn approved_npm_restore_rejects_a_lockfile_changed_during_manager_review() {
    let worktree = std::env::temp_dir().join(format!(
        "koolade-resource-authorized-changed-lock-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&worktree).unwrap();
    let lockfile = |version: &str, integrity: &str| {
        serde_json::to_vec(&serde_json::json!({
            "lockfileVersion": 3,
            "packages": {
                "node_modules/zod": {
                    "version": version,
                    "resolved": format!("https://registry.npmjs.org/zod/-/zod-{version}.tgz"),
                    "integrity": integrity,
                }
            }
        }))
        .unwrap()
    };
    fs::write(
        worktree.join("package-lock.json"),
        lockfile(
            "4.0.0",
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        ),
    )
    .unwrap();
    commit_baseline(&worktree);

    let (bridge, updates) = start_bridge(&worktree, Some("TASK-RESTORE"));
    let mut client = UnixStream::connect(bridge.socket_path()).unwrap();
    serde_json::to_writer(
        &mut client,
        &ResourceRequest {
            action: super::super::ResourceAction::DependencyRequest,
            manager: None,
            url: None,
            dependency: Some(DependencyNeed {
                ecosystem: PackageEcosystem::Npm,
                package: None,
                version: None,
                source: Some("https://registry.npmjs.org".into()),
                command: "npm ci --no-audit".into(),
                reason: "Restore the task project's locked npm dependencies".into(),
                kind: DependencyKind::ExistingRestore,
                lockfile_identity: None,
                introduced_packages: Vec::new(),
            }),
            dependency_request_id: None,
            retry_succeeded: None,
            purpose: "Restore the checked npm lockfile".into(),
        },
    )
    .unwrap();
    client.write_all(b"\n").unwrap();

    let request = take_dependency_update(&updates);
    assert_eq!(request.status, DependencyRequestStatus::ManagerReviewing);
    fs::write(
        worktree.join("package-lock.json"),
        lockfile(
            "4.1.0",
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        ),
    )
    .unwrap();
    answer(
        &request,
        DependencyDecision::AuthorizeForTask,
        None,
        "Man.ager approved the exact public lockfile restore.",
    );

    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    let response: ResourceResponse = serde_json::from_str(&response).unwrap();
    assert_eq!(response.status, "needs_attention");
    assert!(response.summary.contains("changed after authorization"));
    assert_eq!(
        response
            .dependency_request
            .as_ref()
            .map(|request| request.category),
        Some(crate::harness::DependencyFailureCategory::DependencyIntegrityFailure)
    );
    assert_eq!(
        response
            .dependency_result
            .as_ref()
            .and_then(|result| result.status),
        Some(crate::harness::DependencyPreparationStatus::IntegrityFailure)
    );

    drop(bridge);
    fs::remove_dir_all(worktree).unwrap();
}

fn commit_baseline(root: &Path) {
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "package-lock.json"],
        vec![
            "-c",
            "user.name=Synthetic Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "baseline lockfile",
        ],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(a >> 2) as usize] as char);
        output.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
